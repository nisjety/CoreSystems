-- Reverses 0047_space_thread_post_card_admission.up.sql: returns the post_card admission row to the declared state
-- that migration 0045 seeded. It changes that one row and nothing else; the capability row is not touched.

UPDATE agent_action_admissions
SET status = 'declared',
    updated_at = now()
WHERE action_id = 'space.thread.post_card'
  AND status = 'enabled';
