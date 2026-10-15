SELECT MIN(t.title) AS movie_title,
       MIN(mi1.info) AS genre
FROM info_type AS it1,
     info_type AS it2,
     keyword AS k,
     movie_info AS mi1,
     movie_info AS mi2,
     movie_keyword AS mk,
     title AS t
WHERE it1.info = 'genres'
  AND it2.info = 'rating'
  AND k.keyword LIKE '%hero%'
  AND mi1.info IN ('Action', 'Drama')
  AND t.production_year > 1990
  AND it1.id = mi1.info_type_id
  AND it2.id = mi2.info_type_id
  AND t.id = mi1.movie_id
  AND t.id = mi2.movie_id
  AND t.id = mk.movie_id
  AND k.id = mk.keyword_id
  AND mi1.movie_id = mk.movie_id;
