-- DEVELOPMENT ONLY. Restores the documented demo credentials that the
-- 20260926000000_revoke_seeded_credentials migration clears:
--   admin / password123   (admin)
--   user1 / user123
--   user2 / user456
-- Required by tests/postman_collection.json and tests/fingest_performance_test.jmx.
--
--   psql "$DATABASE_URL" -f scripts/seed_dev_credentials.sql
UPDATE account SET password = '$2b$12$6bElbYoERmMOp1tTas9qQeFdDeWEEa53me2grUXNXbpKVrgx2HWsa'
WHERE login = 'admin';
UPDATE account SET password = '$2b$12$/iKqLMLGOp71oZTtoSw1LOgizw/HkCUULMrzfi.yF5SLpATXiVugK'
WHERE login = 'user1';
UPDATE account SET password = '$2b$12$s4FWvZCdZGWUPjKK97yvS.yVv2eLfncP8inckkjCJJOX.x6Uuw5AO'
WHERE login = 'user2';
