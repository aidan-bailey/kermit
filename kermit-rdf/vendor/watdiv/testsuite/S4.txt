#mapping v1 wsdbm:AgeGroup uniform
SELECT ?v0 ?v2 ?v3 WHERE {
	?v0	foaf:age	%v1% .
	?v0	foaf:familyName	?v2 .
	?v3	mo:artist	?v0 .
	?v0	sorg:nationality	wsdbm:Country1 .
}

