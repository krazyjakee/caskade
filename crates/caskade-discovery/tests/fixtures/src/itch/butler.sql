-- Original butler schema subset, current squash platform columns and independent cave IDs.
CREATE TABLE install_locations (id TEXT PRIMARY KEY, path TEXT);
CREATE TABLE games (id INTEGER PRIMARY KEY, title TEXT);
CREATE TABLE uploads (id INTEGER PRIMARY KEY, windows TEXT, linux TEXT, osx TEXT);
CREATE TABLE caves (id TEXT PRIMARY KEY, game_id INTEGER, upload_id INTEGER, install_location_id TEXT, install_folder_name TEXT, custom_install_folder TEXT, seconds_run INTEGER, installed_size INTEGER);
INSERT INTO install_locations VALUES('fixture-location','@GAMES@');
INSERT INTO games VALUES(101,'Fixture Itch Portable'),(102,'Fixture Itch Windows'),(103,'Missing Fixture');
INSERT INTO uploads VALUES(1001,'all','all','all'),(1002,'amd64',NULL,NULL);
INSERT INTO caves VALUES('cave-portable',101,1001,'fixture-location','portable',NULL,3600,4096);
INSERT INTO caves VALUES('cave-windows',102,1002,NULL,NULL,'@GAMES@/windows',120,8192);
INSERT INTO caves VALUES('cave-missing',103,NULL,'fixture-location','missing',NULL,0,0);
INSERT INTO caves VALUES('cave-receipt',0,NULL,'fixture-location','receipt-only',NULL,30,1024);
INSERT INTO caves VALUES('cave-escape',101,1001,'fixture-location','../escape',NULL,0,0);
INSERT INTO caves VALUES('bad/id',101,1001,'fixture-location','portable',NULL,0,0);
