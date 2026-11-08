--
-- PostgreSQL database dump
--

\restrict 54MIhqibi3bOTiDtXng4TqEQ0eXUNflcb3tpCMUh7P0N5rsxoomxUzFq1DY30aN

-- Dumped from database version 15.18
-- Dumped by pg_dump version 15.18

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Data for Name: auth_schema_migrations; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.auth_schema_migrations (version, checksum, applied_at) FROM stdin;
init_better_auth.sql	9d2492d1d5d3c3f76f47b537a650e0f58e36276876a3b23378d76330f40d3034	2026-09-03 18:59:05.130615+00
gdpr_hard_delete.sql	b0a345cf67494bd13a3dae68f3a8250367fc7ba73958727c93ffddb962e03799	2026-09-03 18:59:05.837143+00
014_normalized_identity_email.sql	8d8a92a0f0b98a3d5bd51236ed90d2653cadf11a460ba786ff3c6a9e6be3eeaf	2026-09-03 18:59:05.871562+00
015_organization_projection_outbox.sql	c1543706fa1128d3a32207b66c2874730e8c0953773ee89e29b686a5fe1b710c	2026-09-03 18:59:05.931285+00
016_unique_organization_membership.sql	e40899af8b353213cdaba2bdc7b34058969e527842bf99d7b896575ef6678c51	2026-09-03 18:59:06.118167+00
017_invitation_acceptance_repair.sql	b21e7e7f3234ae9cc2f404dc32800fec91343e76eed5e93bc5c9fb479ebab838	2026-09-03 18:59:06.186606+00
018_historical_owner_preflight.sql	efca73707dc10809aba0b63fabeae41ca366aa9f2b49b185922c987e5985b4c8	2026-09-03 18:59:06.259205+00
019_membership_audit_outbox.sql	7467243c2872b8bcbc2a63de21ccb9152af2b37a78b819f2da33c3a7205fe266	2026-09-03 18:59:06.366779+00
020_auth_identity_event_outbox.sql	97361ff5d7f797f7473dc8a198ebc44c20a4a2c83bbab43fde130e97a896fd81	2026-09-03 18:59:06.478862+00
021_invitation_created_at.sql	b0df64181117f4473e32d692d1902ba4c896fa9665ff7af7da7eec11e8d215b3	2026-09-03 18:59:06.579697+00
022_membership_audit_actor_recovery.sql	8747c62c1f4118eed33fe9ca87cfbe05b1148ca33ef082257b4e61c9daa5a159	2026-09-03 18:59:06.605763+00
023_gdpr_outbox_erasure.sql	54d70e6ca03c21e4947af00919dd5fab4f278fa51cb82f6056f483b274b4fdd6	2026-09-03 18:59:06.635893+00
024_revisioned_cross_plane_projection.sql	5ea040490054dc029379bc143ff2f937dd617bd96c486d4b5880a8f4961b917e	2026-09-03 18:59:06.677988+00
025_gdpr_outbox_publish_fence.sql	6f13b9ac7b7728d71d963f250c3d94efa60c3cef9b8bb35823aa73fa2b30b488	2026-09-03 18:59:06.704958+00
026_invitation_removal_preflight.sql	4bc2550f77f4ab62beab8fad55e176def035bc8e30e20c30f5c336678e5b90ec	2026-09-03 18:59:06.736161+00
027_membership_mutation_intents.sql	d6563700aa629fd56e37faeafaff9abde5608d2634ef625d0fa182e638ea6600	2026-09-03 18:59:06.762513+00
028_org_group_grants.sql	86cd05ef8c97d28bb7f53aca6550a37a870722978adc345624d27615fb5bd2df	2026-09-03 18:59:06.841603+00
\.


--
-- PostgreSQL database dump complete
--

\unrestrict 54MIhqibi3bOTiDtXng4TqEQ0eXUNflcb3tpCMUh7P0N5rsxoomxUzFq1DY30aN

