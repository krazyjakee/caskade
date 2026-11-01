-- Independent Flatpak database; local row ids can collide with native.
CREATE TABLE games (id INTEGER PRIMARY KEY,name TEXT,slug TEXT,runner TEXT,directory TEXT,installed INTEGER,configpath TEXT,service TEXT,service_id TEXT,playtime REAL,lastplayed INTEGER);
INSERT INTO games VALUES(1,'Fixture Adventure Flatpak','fixture-adventure','wine','/home/fixture/Games/Fixture Adventure',1,'adventure-flatpak','gog','1207658924',3.0,1699999500);
INSERT INTO games VALUES(2,'Flatpak Only','flatpak-only','linux','/home/fixture/Games/Flatpak Only',1,'flatpak-only',NULL,NULL,0.5,0);
