DELETE FROM notification_channel_configs
WHERE org_id = '_default'
  AND event_type IN (
      'space.mention', 'space.reply', 'space.invite', 'space.agent_done',
      'space.agent_failed', 'space.approval_needed', 'space.agent_awaiting',
      'space.reminder'
  );
