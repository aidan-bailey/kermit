#mapping v3 wsdbm:Topic uniform
SELECT ?v0 ?v1 ?v2 ?v4 ?v5 ?v6 ?v7 ?v8 WHERE {
	?v0	foaf:homepage	?v1 .
	?v2	gr:includes	?v0 .
	?v0	og:tag	%v3% .
	?v0	sorg:description	?v4 .
	?v0	sorg:contentSize	?v8 .
	?v1	sorg:url	?v5 .
	?v1	wsdbm:hits	?v6 .
	?v1	sorg:language	wsdbm:Language0 .
	?v7	wsdbm:likes	?v0 .
}

