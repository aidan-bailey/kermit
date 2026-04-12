# Star Join Example

A **star join** connects multiple relations through a shared "hub" variable. Each
relation contributes one arm of the star, and the hub variable appears in every
relation. This is a common pattern in data-warehouse schemas (fact table joined
to multiple dimension tables).

## Schema

| Relation   | Columns              | Description              |
|------------|----------------------|--------------------------|
| works_in   | person, department   | Person to department map |
| earns      | person, salary_band  | Person to salary band    |
| located    | person, office       | Person to office          |

**Person** is the hub variable — it appears in all three relations.

## Query

```datalog
star(Person, Dept, Salary, Office) :-
  works_in(Person, Dept),
  earns(Person, Salary),
  located(Person, Office).
```

## Expected Output

Every person appears in all three relations, so the join produces 5 result tuples:

```
1,10,100,1000
2,10,200,1000
3,20,100,2000
4,20,300,2000
5,30,200,3000
```

## Running

```bash
bash kermit/examples/star/run.sh
```

The script runs the join with both `tree-trie` and `column-trie` index structures,
then benchmarks each using Criterion.
