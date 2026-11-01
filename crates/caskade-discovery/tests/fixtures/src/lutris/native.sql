-- Original minimal Lutris schema; playtime is decimal hours.
CREATE TABLE games (id INTEGER PRIMARY KEY,name TEXT,slug TEXT,runner TEXT,executable TEXT,directory TEXT,installed INTEGER,configpath TEXT,service TEXT,service_id TEXT,playtime REAL,lastplayed INTEGER);
INSERT INTO games VALUES(1,'Fixture Puzzle','fixture-puzzle','linux',NULL,'/home/fixture/Games/Fixture Puzzle',1,'puzzle-config','steam','620',1.5,1699999000);
INSERT INTO games VALUES(2,'Fixture Adventure','fixture-adventure','wine',NULL,'/home/fixture/Games/Fixture Adventure',1,'adventure-config','gog','1207658924',2.25,1699998000);
INSERT INTO games VALUES(3,'Uninstalled Catalog Game','uninstalled','wine',NULL,NULL,0,'absent',NULL,NULL,0,0);
INSERT INTO games VALUES(4,'Local Café','local-cafe','linux','game','/home/fixture/Games/Local Café',1,'../../private',NULL,NULL,-1,0);
