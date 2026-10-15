-- Hand-written IMDb-shaped schema for the JOB generator tests. Column names follow
-- the Join Order Benchmark schema for the columns used here; nothing is copied.
CREATE TABLE title (
    id integer NOT NULL PRIMARY KEY,
    title text NOT NULL,
    kind_id integer NOT NULL,
    production_year integer
);

CREATE TABLE kind_type (
    id integer NOT NULL PRIMARY KEY,
    kind text NOT NULL
);

CREATE TABLE keyword (
    id integer NOT NULL PRIMARY KEY,
    keyword text NOT NULL
);

CREATE TABLE movie_keyword (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    keyword_id integer NOT NULL
);

CREATE TABLE name (
    id integer NOT NULL PRIMARY KEY,
    name text NOT NULL
);

-- person_id is nullable here (NOT NULL in JOB) so the fixture has a NULL join key.
CREATE TABLE cast_info (
    id integer NOT NULL PRIMARY KEY,
    person_id integer,
    movie_id integer NOT NULL
);

CREATE TABLE info_type (
    id integer NOT NULL PRIMARY KEY,
    info text NOT NULL
);

-- info_type_id is nullable here (NOT NULL in JOB), for the same reason.
CREATE TABLE movie_info (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    info_type_id integer,
    info text NOT NULL
);
