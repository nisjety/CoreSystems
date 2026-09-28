DELETE FROM notification_channel_configs
WHERE org_id = '_default'
  AND event_type = 'chat.mail_task_drafted';
