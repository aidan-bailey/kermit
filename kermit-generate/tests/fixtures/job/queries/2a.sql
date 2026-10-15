SELECT MIN(t.title) AS movie_title,
       MIN(n.name) AS actor
FROM cast_info AS ci,
     kind_type AS kt,
     name AS n,
     title AS t
WHERE kt.kind = 'movie'
  AND n.name LIKE '%Ray%'
  AND t.title LIKE '%"Hero"%'
  AND ci.movie_id = t.id
  AND ci.person_id = n.id
  AND kt.id = t.kind_id;
