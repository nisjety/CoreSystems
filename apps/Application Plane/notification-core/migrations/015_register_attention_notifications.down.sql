DELETE FROM notification_channel_configs
WHERE org_id = '_default'
  AND event_type IN ('modelplane.needs_approval', 'modelplane.needs_input', 'modelplane.plan_ready');
