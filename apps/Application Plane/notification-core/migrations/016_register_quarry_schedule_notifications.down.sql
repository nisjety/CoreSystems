DELETE FROM notification_channel_configs
WHERE org_id = '_default'
  AND event_type IN ('quarry.schedule_run_skipped', 'quarry.schedule_run_failed');
