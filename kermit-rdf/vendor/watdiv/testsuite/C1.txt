SELECT ?v0 ?v4 ?v6 ?v7 WHERE {
	?v0	sorg:caption	?v1 .
	?v0	sorg:text	?v2 .
	?v0	sorg:contentRating	?v3 .
	?v0	rev:hasReview	?v4 .
	?v4	rev:title	?v5 .
	?v4	rev:reviewer	?v6 .
	?v7	sorg:actor	?v6 .
	?v7	sorg:language	?v8 .
}

