#mapping v1 wsdbm:Topic uniform
SELECT ?v0 ?v2 ?v3 ?v4 ?v5 WHERE {
	?v0	og:tag	%v1% .
	?v0	rdf:type	?v2 .
	?v3	sorg:trailer	?v4 .
	?v3	sorg:keywords	?v5 .
	?v3	wsdbm:hasGenre	?v0 .
	?v3	rdf:type	wsdbm:ProductCategory2 .
}

