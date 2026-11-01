CREATE TABLE InstalledBaseProducts(productId INTEGER,installationPath TEXT,installationDate TEXT);
CREATE TABLE LimitedDetails(id INTEGER,productId INTEGER,title TEXT);
CREATE TABLE GameTimes(releaseKey TEXT,minutesInGame INTEGER);
CREATE TABLE LastPlayedDates(gameReleaseKey TEXT,lastPlayedDate TEXT);
INSERT INTO InstalledBaseProducts VALUES(1207658924,'C:\GOG Games\The Witcher','2024-01-01');
INSERT INTO LimitedDetails VALUES(1,1207658924,'The Witcher');
INSERT INTO GameTimes VALUES('gog_1207658924',12);
