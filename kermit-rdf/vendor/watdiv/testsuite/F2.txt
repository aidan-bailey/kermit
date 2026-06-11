#mapping v8 wsdbm:SubGenre uniform
SELECT ?v0 ?v1 ?v2 ?v4 ?v5 ?v6 ?v7 WHERE {
	?v0	foaf:homepage	?v1 .
	?v0	og:title	?v2 .
	?v0	rdf:type	?v3 .
	?v0	sorg:caption	?v4 .
	?v0	sorg:description	?v5 .
	?v1	sorg:url	?v6 .
	?v1	wsdbm:hits	?v7 .
	?v0	wsdbm:hasGenre	%v8% .
}

