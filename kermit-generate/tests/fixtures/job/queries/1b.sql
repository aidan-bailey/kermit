SELECT MIN(t.title) AS movie_title,
       MIN(mi1.info) AS genre
FROM info_type AS it1,
     keyword AS k,
     movie_info AS mi1,
     movie_keyword AS mk,
     title AS t
WHERE it1.info = 'genres'
  AND k.keyword LIKE '%hero%'
  AND mi1.info NOT LIKE 'Doc%'
  AND t.production_year BETWEEN 1980 AND 1995
  AND it1.id = mi1.info_type_id
  AND t.id = mi1.movie_id
  AND t.id = mk.movie_id
  AND k.id = mk.keyword_id;
