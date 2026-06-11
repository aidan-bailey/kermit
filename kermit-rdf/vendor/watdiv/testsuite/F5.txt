#mapping v2 wsdbm:Retailer uniform
SELECT ?v0 ?v1 ?v3 ?v4 ?v5 ?v6 WHERE {
	?v0	gr:includes	?v1 .
	%v2%	gr:offers	?v0 .
	?v0	gr:price	?v3 .
	?v0	gr:validThrough	?v4 .
	?v1	og:title	?v5 .
	?v1	rdf:type	?v6 .
}

