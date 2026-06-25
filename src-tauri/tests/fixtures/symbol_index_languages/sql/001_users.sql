\i ./extensions.sql

CREATE SCHEMA app;

CREATE TABLE IF NOT EXISTS app.users (
    id uuid PRIMARY KEY,
    email text NOT NULL
);

CREATE OR REPLACE VIEW app.active_users AS
SELECT * FROM app.users WHERE active = true;

CREATE UNIQUE INDEX idx_users_email ON app.users(email);

CREATE OR REPLACE FUNCTION app.normalize_user_id(id text)
RETURNS text AS $$
SELECT trim(id);
$$ LANGUAGE sql;

CREATE TRIGGER users_updated_at
BEFORE UPDATE ON app.users
FOR EACH ROW EXECUTE FUNCTION app.touch_updated_at();
