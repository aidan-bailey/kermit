#mapping v0 wsdbm:City uniform
SELECT ?v1 ?v2 WHERE {
	%v0%	gn:parentCountry	?v1 .
	?v2	wsdbm:likes	wsdbm:Product0 .
	?v2	sorg:nationality	?v1 .
}

