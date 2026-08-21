DELETE FROM notification_channel_configs
WHERE org_id = '_default'
  AND event_type IN ('modelplane.run_completed', 'modelplane.run_failed');
