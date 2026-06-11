#mapping v1 wsdbm:ProductCategory uniform
SELECT ?v0 ?v2 ?v3 ?v4 WHERE {
	?v0	rdf:type	%v1% .
	?v0	sorg:caption	?v2 .
	?v0	wsdbm:hasGenre	?v3 .
	?v0	sorg:publisher	?v4 .
}

