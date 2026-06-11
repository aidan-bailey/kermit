#mapping v3 wsdbm:SubGenre uniform
SELECT ?v0 ?v1 ?v2 ?v4 ?v5 ?v6 WHERE {
	?v0	sorg:contentRating	?v1 .
	?v0	sorg:contentSize	?v2 .
	?v0	wsdbm:hasGenre	%v3% .
	?v4	wsdbm:makesPurchase	?v5 .
	?v5	wsdbm:purchaseDate	?v6 .
	?v5	wsdbm:purchaseFor	?v0 .
}

