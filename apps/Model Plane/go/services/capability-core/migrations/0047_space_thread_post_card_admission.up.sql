-- 0047_space_thread_post_card_admission.up.sql
--
-- W3-DEC-CARD (owner decision D3, 2026-11-16): agent card posting is enabled. This flips the admission row
-- space.thread.post_card, which migration 0045 seeded as declared, to enabled. The row keeps the shape that
-- rename and pin were admitted with in 0045 (control_run_decision, owner_contract hash basis, no approval, the
-- golden schema hash); only its status changes.
--
-- A row alone admits nothing. The capability row cap.tool.space.thread.post_card stays unavailable until
-- execution-core attests it on the owner-action lane after its Session Core probe, Control signs a run-action
-- decision for it, the golden registry agrees (phase 1, the same hash), and capability-core runs with
-- MODEL_ACTION_ADMISSION=table.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot. The guard flips only a row that is still
-- declared, so an enabled row is untouched and a retired row is never revived. To switch post_card off, set its
-- row to retired: a declared row would be flipped again at the next boot.

UPDATE agent_action_admissions
SET status = 'enabled',
    updated_at = now()
WHERE action_id = 'space.thread.post_card'
  AND status = 'declared';
