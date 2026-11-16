-- Reverses 0046_space_canvas_capabilities.up.sql. It removes the two room canvas capability rows the migration
-- created, and nothing else. A capability an operator has since adopted under another creator is kept.

DELETE FROM capability_scopes
WHERE capability_id IN ('cap.tool.space.canvas.read', 'cap.tool.space.canvas.write');
DELETE FROM capability_health
WHERE capability_id IN ('cap.tool.space.canvas.read', 'cap.tool.space.canvas.write');
DELETE FROM capability_versions
WHERE capability_id IN ('cap.tool.space.canvas.read', 'cap.tool.space.canvas.write');
DELETE FROM skill_packages
WHERE capability_id IN ('cap.tool.space.canvas.read', 'cap.tool.space.canvas.write');

DELETE FROM capabilities
WHERE id IN ('cap.tool.space.canvas.read', 'cap.tool.space.canvas.write')
  AND created_by = 'migration:0046_space_canvas_capabilities';
