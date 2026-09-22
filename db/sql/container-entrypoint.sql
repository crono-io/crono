-- Run the canonical bootstrap from PostgreSQL's container init directory while
-- preserving relative \ir includes in db/sql/00_init.sql.
\i /db/sql/00_init.sql
