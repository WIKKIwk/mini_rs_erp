use super::*;
use crate::telegram::alerts::models::{AlertMember, MemberPage};
use ferogram::{ParticipantStatus, tl};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub(crate) const MEMBER_PAGE_SIZE: usize = 20;
const CLIENT_TTL: Duration = Duration::from_secs(300);
const LIST_TTL: Duration = Duration::from_secs(60);

type ClientSlot = Arc<Mutex<Option<Arc<LookupClient>>>>;
type GroupKey = (String, String);

#[derive(Default)]
pub(super) struct LookupCache {
    clients: Mutex<BTreeMap<String, ClientSlot>>,
}

struct LookupClient {
    client: Client,
    shutdown: ShutdownToken,
    fingerprint: [u8; 32],
    created: Instant,
    groups: Mutex<Option<(Instant, Vec<TelegramUserGroup>)>>,
    basic_members: Mutex<BTreeMap<GroupKey, (Instant, Vec<AlertMember>)>>,
    member_pages: Mutex<BTreeMap<(GroupKey, String, usize), (Instant, MemberPage)>>,
}

impl Drop for LookupClient {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl TelegramUserAccountService {
    pub(super) async fn clear_lookup_cache(&self, sender: &str) {
        self.lookup_cache.clients.lock().await.remove(sender);
    }

    async fn lookup_client(&self, sender: &str) -> Result<Arc<LookupClient>, UserAccountError> {
        let session = self
            .store
            .user_session(sender)
            .await
            .map_err(map_store)?
            .ok_or(UserAccountError::NotAuthorized)?;
        let (api_id, api_hash) = api_credentials(&self.store).await?;
        let fingerprint: [u8; 32] =
            Sha256::digest(format!("{api_id}:{api_hash}:{session}").as_bytes()).into();
        let slot = self
            .lookup_cache
            .clients
            .lock()
            .await
            .entry(sender.into())
            .or_default()
            .clone();
        // One connection per profile; simultaneous keystrokes share the same cold start.
        let mut slot = slot.lock().await;
        if let Some(client) = slot
            .as_ref()
            .filter(|c| c.fingerprint == fingerprint && c.created.elapsed() < CLIENT_TTL)
        {
            return Ok(client.clone());
        }
        let (client, shutdown) = self.authorized_client(sender).await?;
        let cached = Arc::new(LookupClient {
            client,
            shutdown,
            fingerprint,
            created: Instant::now(),
            groups: Mutex::new(None),
            basic_members: Mutex::new(BTreeMap::new()),
            member_pages: Mutex::new(BTreeMap::new()),
        });
        *slot = Some(cached.clone());
        Ok(cached)
    }

    pub(super) async fn cached_writable_groups(
        &self,
        sender: &str,
    ) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
        let cached = self.lookup_client(sender).await?;
        cached_groups(&cached).await
    }

    pub(crate) async fn alert_member_search(
        &self,
        sender: &str,
        group: &TelegramUserGroup,
        query: &str,
        offset: usize,
    ) -> Result<MemberPage, UserAccountError> {
        if offset > 100_000 {
            return Err(UserAccountError::GroupNotWritable);
        }
        let cached = self.lookup_client(sender).await?;
        let key = (group.chat_type.clone(), group.chat_id.clone());
        let query = query.trim().trim_start_matches('@').to_lowercase();
        let page_key = (key.clone(), query.clone(), offset);
        if let Some((_, page)) = cached
            .member_pages
            .lock()
            .await
            .get(&page_key)
            .filter(|(at, _)| at.elapsed() < LIST_TTL)
        {
            return Ok(page.clone());
        }
        let groups = cached_groups(&cached).await?;
        if !groups
            .iter()
            .any(|g| g.chat_id == group.chat_id && g.chat_type == group.chat_type)
        {
            return Err(UserAccountError::GroupNotWritable);
        }
        // Dialogs above populate the client's peer/access-hash cache once.
        let id = group
            .chat_id
            .parse()
            .map_err(|_| UserAccountError::GroupNotWritable)?;
        let peer = match group.chat_type.as_str() {
            "group" => tl::types::PeerChat { chat_id: id }.into(),
            "supergroup" => tl::types::PeerChannel { channel_id: id }.into(),
            _ => return Err(UserAccountError::GroupNotWritable),
        };
        let input = cached
            .client
            .resolve_to_input_peer(&peer)
            .await
            .map_err(map_transport)?;
        let page = match input {
            tl::enums::InputPeer::Channel(channel) => {
                let filter = if query.is_empty() {
                    tl::enums::ChannelParticipantsFilter::ChannelParticipantsRecent
                } else {
                    tl::types::ChannelParticipantsSearch { q: query.clone() }.into()
                };
                let result = cached
                    .client
                    .invoke(&tl::functions::channels::GetParticipants {
                        channel: tl::types::InputChannel {
                            channel_id: channel.channel_id,
                            access_hash: channel.access_hash,
                        }
                        .into(),
                        filter,
                        offset: offset as i32,
                        limit: (MEMBER_PAGE_SIZE + 1) as i32,
                        hash: 0,
                    })
                    .await
                    .map_err(map_transport)?;
                let tl::enums::channels::ChannelParticipants::ChannelParticipants(result) = result
                else {
                    return Ok(MemberPage {
                        members: vec![],
                        has_more: false,
                    });
                };
                let has_more = result.participants.len() > MEMBER_PAGE_SIZE;
                let ids: Vec<_> = result
                    .participants
                    .into_iter()
                    .take(MEMBER_PAGE_SIZE)
                    .filter_map(|p| match p {
                        tl::enums::ChannelParticipant::ChannelParticipant(p) => Some(p.user_id),
                        tl::enums::ChannelParticipant::ParticipantSelf(p) => Some(p.user_id),
                        tl::enums::ChannelParticipant::Creator(p) => Some(p.user_id),
                        tl::enums::ChannelParticipant::Admin(p) => Some(p.user_id),
                        _ => None,
                    })
                    .collect();
                let members = result
                    .users
                    .into_iter()
                    .filter_map(|u| match u {
                        tl::enums::User::User(u) if ids.contains(&u.id) => member_from_user(u),
                        _ => None,
                    })
                    .collect();
                MemberPage { members, has_more }
            }
            _ => {
                let mut roster = cached.basic_members.lock().await;
                if !roster
                    .get(&key)
                    .is_some_and(|(at, _)| at.elapsed() < LIST_TTL)
                {
                    let users = cached
                        .client
                        .get_participants(peer, 0)
                        .await
                        .map_err(map_transport)?;
                    let members = users
                        .into_iter()
                        .filter(|p| {
                            !matches!(
                                p.status,
                                ParticipantStatus::Left | ParticipantStatus::Banned
                            )
                        })
                        .filter_map(|p| member_from_user(p.user))
                        .collect();
                    roster.insert(key.clone(), (Instant::now(), members));
                }
                filter_member_page(&roster[&key].1, &query, offset)
            }
        };
        let mut pages = cached.member_pages.lock().await;
        pages.retain(|_, (at, _)| at.elapsed() < LIST_TTL);
        if pages.len() >= 128 {
            pages.clear();
        }
        pages.insert(page_key, (Instant::now(), page.clone()));
        Ok(page)
    }
}

async fn cached_groups(cached: &LookupClient) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
    let mut groups = cached.groups.lock().await;
    if let Some((_, value)) = groups.as_ref().filter(|(at, _)| at.elapsed() < LIST_TTL) {
        return Ok(value.clone());
    }
    let value = list_writable_groups(&cached.client).await?;
    *groups = Some((Instant::now(), value.clone()));
    Ok(value)
}

fn member_from_user(user: tl::types::User) -> Option<AlertMember> {
    if user.bot || user.deleted || user.id <= 0 {
        return None;
    }
    Some(AlertMember {
        user_id: user.id,
        display_name: format!(
            "{} {}",
            user.first_name.unwrap_or_default(),
            user.last_name.unwrap_or_default()
        )
        .trim()
        .to_string(),
        username: user.username.unwrap_or_default(),
        access_hash: user.access_hash,
    })
}

fn filter_member_page(members: &[AlertMember], query: &str, offset: usize) -> MemberPage {
    let query = query.trim().trim_start_matches('@').to_lowercase();
    let found: Vec<_> = members
        .iter()
        .filter(|m| {
            query.is_empty()
                || m.display_name.to_lowercase().contains(&query)
                || m.username.to_lowercase().contains(&query)
                || m.user_id.to_string() == query
        })
        .skip(offset)
        .take(MEMBER_PAGE_SIZE + 1)
        .cloned()
        .collect();
    MemberPage {
        has_more: found.len() > MEMBER_PAGE_SIZE,
        members: found.into_iter().take(MEMBER_PAGE_SIZE).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_group_members_search_by_name_username_and_id_with_pagination() {
        let members: Vec<_> = (1..=25)
            .map(|id| AlertMember {
                user_id: id,
                display_name: format!("Qolipchi {id}"),
                username: format!("User{id}"),
                access_hash: None,
            })
            .collect();
        assert_eq!(
            filter_member_page(&members, "@uSER25", 0).members[0].user_id,
            25
        );
        assert_eq!(filter_member_page(&members, "25", 0).members[0].user_id, 25);
        let first = filter_member_page(&members, "QOLIPCHI", 0);
        assert_eq!(first.members.len(), MEMBER_PAGE_SIZE);
        assert!(first.has_more);
        let next = filter_member_page(&members, "", MEMBER_PAGE_SIZE);
        assert_eq!(next.members.len(), 5);
        assert!(!next.has_more);
        assert!(
            filter_member_page(&members, "missing", 0)
                .members
                .is_empty()
        );
    }
}
