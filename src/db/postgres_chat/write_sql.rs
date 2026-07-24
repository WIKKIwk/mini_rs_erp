const ORDER_FREEZE_UPDATE_MESSAGE_SQL: &str = r#"UPDATE mini_chat_messages
   SET body = $4,
       message_type = 'order_freeze_request',
       metadata_json = $5,
       edited_at = now()
   WHERE conversation_id = $1
     AND sender_principal_id = $2
     AND client_message_id = $3
   RETURNING
     message_id,
     conversation_id,
     sender_principal_id,
     $6::TEXT AS sender_role,
     $7::TEXT AS sender_ref,
     $8::TEXT AS sender_display_name,
     client_message_id,
     message_sequence,
     message_type,
     body,
     metadata_json,
     NULL::TEXT AS attachment_id,
     NULL::TEXT AS media_id,
     NULL::TEXT AS media_kind,
     NULL::TEXT AS media_content_type,
     NULL::BIGINT AS media_size_bytes,
     NULL::INTEGER AS media_width_pixels,
     NULL::INTEGER AS media_height_pixels,
     NULL::BIGINT AS media_duration_ms,
     EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
     EXTRACT(EPOCH FROM edited_at)::BIGINT AS edited_at_unix,
     NULL::BIGINT AS deleted_at_unix"#;

const ORDER_FREEZE_INSERT_MESSAGE_SQL: &str = r#"INSERT INTO mini_chat_messages
     (message_id, conversation_id, sender_principal_id, client_message_id,
      message_sequence, message_type, body, metadata_json)
   VALUES ($1, $2, $3, $4, $5, 'order_freeze_request', $6, $7)
   RETURNING
     message_id,
     conversation_id,
     sender_principal_id,
     $8::TEXT AS sender_role,
     $9::TEXT AS sender_ref,
     $10::TEXT AS sender_display_name,
     client_message_id,
     message_sequence,
     message_type,
     body,
     metadata_json,
     NULL::TEXT AS attachment_id,
     NULL::TEXT AS media_id,
     NULL::TEXT AS media_kind,
     NULL::TEXT AS media_content_type,
     NULL::BIGINT AS media_size_bytes,
     NULL::INTEGER AS media_width_pixels,
     NULL::INTEGER AS media_height_pixels,
     NULL::BIGINT AS media_duration_ms,
     EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
     NULL::BIGINT AS edited_at_unix,
     NULL::BIGINT AS deleted_at_unix"#;

const ORDER_FREEZE_RECIPIENTS_SQL: &str = r#"SELECT recipient.principal_id, recipient.principal_role, recipient.principal_ref
       FROM mini_chat_conversation_members member
       JOIN mini_chat_principals recipient ON recipient.principal_id = member.principal_id
       WHERE member.conversation_id = $1
         AND member.left_at IS NULL
         AND recipient.principal_role <> 'customer'
         AND recipient.active = TRUE"#;

const CHAT_SENDER_FOR_CONVERSATION_SQL: &str = r#"SELECT p.principal_id, p.principal_role, p.principal_ref, p.display_name, p.avatar_url
       FROM mini_chat_principals p
       JOIN mini_chat_conversation_members member ON member.principal_id = p.principal_id
       WHERE p.principal_role = $1
         AND p.principal_ref = $2
         AND p.principal_role <> 'customer'
         AND member.conversation_id = $3
         AND member.left_at IS NULL
         AND member.can_post = TRUE
         AND p.active = TRUE
         AND NOT EXISTS (
           SELECT 1
           FROM mini_chat_conversation_members customer_member
           JOIN mini_chat_principals customer_principal
             ON customer_principal.principal_id = customer_member.principal_id
           WHERE customer_member.conversation_id = member.conversation_id
             AND customer_member.left_at IS NULL
             AND customer_principal.principal_role = 'customer'
         )"#;

const EXISTING_MESSAGE_SQL: &str = r#"SELECT
     m.message_id,
     m.conversation_id,
     m.sender_principal_id,
     sender.principal_role AS sender_role,
     sender.principal_ref AS sender_ref,
     sender.display_name AS sender_display_name,
     m.client_message_id,
     m.message_sequence,
     m.message_type,
     m.body,
     m.metadata_json,
     attachment.attachment_id,
     media.media_id,
     media.media_kind,
     media.processed_content_type AS media_content_type,
     media.processed_size_bytes AS media_size_bytes,
     media.width_pixels AS media_width_pixels,
     media.height_pixels AS media_height_pixels,
     media.duration_ms AS media_duration_ms,
     EXTRACT(EPOCH FROM m.created_at)::BIGINT AS created_at_unix,
     EXTRACT(EPOCH FROM m.edited_at)::BIGINT AS edited_at_unix,
     EXTRACT(EPOCH FROM m.deleted_at)::BIGINT AS deleted_at_unix
   FROM mini_chat_messages m
   JOIN mini_chat_principals sender ON sender.principal_id = m.sender_principal_id
   LEFT JOIN mini_chat_message_attachments attachment ON attachment.message_id = m.message_id
   LEFT JOIN mini_chat_media media ON media.media_id = attachment.media_id
   WHERE m.conversation_id = $1
     AND m.sender_principal_id = $2
     AND m.client_message_id = $3"#;

const CLAIM_ORDER_FREEZE_CHAT_EVENTS_SQL: &str = r#"WITH candidates AS (
     SELECT event_sequence
     FROM mini_order_freeze_chat_outbox
     WHERE delivered_at IS NULL
       AND (locked_until IS NULL OR locked_until < now())
     ORDER BY event_sequence ASC
     FOR UPDATE SKIP LOCKED
     LIMIT $1
   ),
   claimed AS (
     UPDATE mini_order_freeze_chat_outbox event
     SET attempts = event.attempts + 1,
         locked_until = now() + interval '30 seconds'
     FROM candidates
     WHERE event.event_sequence = candidates.event_sequence
     RETURNING event.event_sequence, event.event_id, event.request_id,
               event.status, event.attempts
   )
   SELECT
     claimed.event_sequence,
     claimed.event_id,
     request.request_id,
     claimed.status,
     request.order_id,
     map.order_number,
     map.title AS order_title,
     request.requester_role,
     request.requester_ref,
     request.requester_display_name,
     request.target_session_id,
     request.target_apparatus,
     request.target_worker_role,
     request.target_worker_ref,
     request.target_worker_display_name,
     request.requested_at_unix,
     CASE claimed.status
       WHEN request.status THEN request.transitioned_at_unix
       ELSE request.requested_at_unix
     END AS transitioned_at_unix,
     claimed.attempts
   FROM claimed
   JOIN mini_order_freeze_requests request
     ON request.request_id = claimed.request_id
   JOIN mini_production_maps map ON map.id = request.order_id
   ORDER BY claimed.event_sequence ASC"#;
