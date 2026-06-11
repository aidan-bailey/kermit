SELECT ?v0 ?v3 ?v4 ?v8 WHERE {
	?v0	sorg:legalName	?v1 .
	?v0	gr:offers	?v2 .
	?v2	sorg:eligibleRegion	wsdbm:Country5 .
	?v2	gr:includes	?v3 .
	?v4	sorg:jobTitle	?v5 .
	?v4	foaf:homepage	?v6 .
	?v4	wsdbm:makesPurchase	?v7 .
	?v7	wsdbm:purchaseFor	?v3 .
	?v3	rev:hasReview	?v8 .
	?v8	rev:totalVotes	?v9 .
}

