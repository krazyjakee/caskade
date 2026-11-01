-- Notifications keep their title and body as messages (#214): JSON of
-- `caskade_core::i18n::Message`, translated each time the feed is read.
-- `title` and `body` keep the English beside them. Items from before have
-- no messages and keep showing their English.
ALTER TABLE notifications ADD COLUMN title_message TEXT;
ALTER TABLE notifications ADD COLUMN body_message TEXT;
