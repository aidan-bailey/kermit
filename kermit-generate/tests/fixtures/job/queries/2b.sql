SELECT MIN(t.title) AS movie_title,
       MIN(n.name) AS actor
FROM cast_info AS ci,
     name AS n,
     title AS t
WHERE n.name LIKE '%Zed%'
  AND t.production_year > 1990
  AND ci.movie_id = t.id
  AND ci.person_id = n.id;
