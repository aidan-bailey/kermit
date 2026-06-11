#mapping v1 wsdbm:Website uniform
SELECT ?v0 ?v2 ?v3 WHERE {
	?v0	wsdbm:subscribes	%v1% .
	?v2	sorg:caption	?v3 .
	?v0	wsdbm:likes	?v2 .
}

