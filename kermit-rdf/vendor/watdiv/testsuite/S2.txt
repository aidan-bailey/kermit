#mapping v2 wsdbm:Country uniform
SELECT ?v0 ?v1 ?v3 WHERE {
	?v0	dc:Location	?v1 .
	?v0	sorg:nationality	%v2% .
	?v0	wsdbm:gender	?v3 .
	?v0	rdf:type	wsdbm:Role2 .
}

