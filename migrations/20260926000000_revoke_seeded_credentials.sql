-- The sample-data migration seeded admin/user1/user2 with publicly documented passwords,
-- and migrations run in every environment. Clear those credentials wherever they are
-- still the published ones; `scripts/seed_dev_credentials.sql` restores them locally.
UPDATE account
SET password = NULL
WHERE (login, password) IN (
    ('admin', '$2b$12$6bElbYoERmMOp1tTas9qQeFdDeWEEa53me2grUXNXbpKVrgx2HWsa'),
    ('user1', '$2b$12$/iKqLMLGOp71oZTtoSw1LOgizw/HkCUULMrzfi.yF5SLpATXiVugK'),
    ('user2', '$2b$12$s4FWvZCdZGWUPjKK97yvS.yVv2eLfncP8inckkjCJJOX.x6Uuw5AO')
);
