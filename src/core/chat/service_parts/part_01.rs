
#[derive(Clone)]
pub struct ChatService {
    store: Arc<dyn ChatStorePort>,
    hub: ChatHub,
    delivery_enabled: bool,
}
