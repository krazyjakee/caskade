-- Original synthetic Amazon DbSet subset used for bounded SQLite reader tests.
CREATE TABLE DbSet (Id TEXT PRIMARY KEY, ProductTitle TEXT, InstallDirectory TEXT, Installed INTEGER);
INSERT INTO DbSet VALUES ('fixture-amazon','Fixture Racing','C:\Games\Fixture Racing',1);
