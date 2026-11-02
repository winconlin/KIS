-- Access- und Refresh-Token einer Sitzung gemeinsam widerrufen.
ALTER TABLE sessions ADD COLUMN access_jti TEXT;
CREATE UNIQUE INDEX idx_sessions_access_jti ON sessions(access_jti);
