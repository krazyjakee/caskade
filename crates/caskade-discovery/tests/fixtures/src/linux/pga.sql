-- Original synthetic Lutris-shaped schema and records; no launcher data copied.
CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT NOT NULL, slug TEXT NOT NULL,
 runner TEXT, directory TEXT, installed INTEGER, service TEXT, service_id TEXT,
 playtime REAL DEFAULT 0, lastplayed INTEGER DEFAULT 0);
INSERT INTO games VALUES (1,'Fixture Puzzle','fixture-puzzle','linux','/home/fixture/Games/Fixture Puzzle',1,'steam','620',3600,1700000000);
INSERT INTO games VALUES (2,'Fixture Adventure','fixture-adventure','wine','/home/fixture/Games/Fixture Adventure',1,'gog','1207658924',120,1700000000);
INSERT INTO games VALUES (3,'Not Installed','not-installed','wine','/home/fixture/Games/Absent',0,NULL,NULL,0,0);
