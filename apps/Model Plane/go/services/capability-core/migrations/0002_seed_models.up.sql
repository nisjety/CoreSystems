-- Slice 10: seed default foundation models.
INSERT INTO models (id, org_id, scope, provider, name, version, config_json, enabled, risk_level, lazy_load, description)
VALUES
    ('11111111-1111-4111-8111-111111111111', NULL, 'global', 'openai',    'gpt-4o',              '2024-08-06', '{}'::jsonb, true, 'low',  false, 'OpenAI GPT-4o foundation model.'),
    ('22222222-2222-4222-8222-222222222222', NULL, 'global', 'openai',    'gpt-4o-mini',         '2024-07-18', '{}'::jsonb, true, 'low',  false, 'OpenAI GPT-4o mini foundation model.'),
    ('33333333-3333-4333-8333-333333333333', NULL, 'global', 'anthropic', 'claude-3-5-sonnet',   '20241022',   '{}'::jsonb, true, 'low',  false, 'Anthropic Claude 3.5 Sonnet foundation model.'),
    ('44444444-4444-4444-8444-444444444444', NULL, 'global', 'anthropic', 'claude-3-5-haiku',    '20241022',   '{}'::jsonb, true, 'low',  false, 'Anthropic Claude 3.5 Haiku foundation model.'),
    ('55555555-5555-4555-8555-555555555555', NULL, 'global', 'google',    'gemini-1.5-pro',      '002',        '{}'::jsonb, true, 'low',  false, 'Google Gemini 1.5 Pro foundation model.')
ON CONFLICT DO NOTHING;
