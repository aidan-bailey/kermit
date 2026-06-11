#mapping v2 wsdbm:Retailer uniform
SELECT ?v0 ?v1 ?v3 ?v4 ?v5 ?v6 ?v7 ?v8 ?v9 WHERE {
	?v0	gr:includes	?v1 .
	%v2%	gr:offers	?v0 .
	?v0	gr:price	?v3 .
	?v0	gr:serialNumber	?v4 .
	?v0	gr:validFrom	?v5 .
	?v0	gr:validThrough	?v6 .
	?v0	sorg:eligibleQuantity	?v7 .
	?v0	sorg:eligibleRegion	?v8 .
	?v0	sorg:priceValidUntil	?v9 .
}

