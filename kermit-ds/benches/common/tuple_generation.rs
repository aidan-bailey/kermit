use {
    num_traits::PrimInt,
    rand::{
        distr::{uniform::SampleUniform, Uniform},
        rng, Rng,
    },
    std::{collections::HashSet, hash::Hash},
};

pub fn generate_exponential_tuples<T>(k: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let k_usize = num_traits::cast::<T, usize>(k).expect("Failed to cast T to usize");
    let mut tuples: Vec<Vec<T>> = vec![];

    let tuple = (0..k_usize)
        .map(|_| num_traits::cast::<usize, T>(0).unwrap())
        .collect::<Vec<T>>();

    fn recurse<T>(k_curr: usize, k: usize, current: Vec<T>, result: &mut Vec<Vec<T>>)
    where
        T: PrimInt + num_traits::NumCast,
    {
        if k_curr == k {
            result.push(current);
            return;
        }

        for i in 0..k {
            let mut new_tuple = current.clone();
            new_tuple.push(num_traits::cast::<usize, T>(i).unwrap());
            recurse(k_curr + 1, k, new_tuple, result);
        }
    }

    recurse(0, k_usize, tuple, &mut tuples);

    tuples
}

pub fn generate_factorial_tuples<T>(k: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let k_usize = num_traits::cast::<T, usize>(k).expect("Failed to cast T to usize");
    let mut tuples: Vec<Vec<T>> = vec![];

    let tuple = (0..k_usize)
        .map(|_| num_traits::cast::<usize, T>(0).unwrap())
        .collect::<Vec<T>>();

    fn recurse<T>(k_curr: usize, k: usize, current: Vec<T>, result: &mut Vec<Vec<T>>)
    where
        T: PrimInt + num_traits::NumCast,
    {
        if k_curr == k {
            result.push(current);
            return;
        }

        for i in 0..=k_curr {
            let mut new_tuple = current.clone();
            new_tuple.push(num_traits::cast::<usize, T>(i).unwrap());
            recurse(k_curr + 1, k, new_tuple, result);
        }
    }

    recurse(0, k_usize, tuple, &mut tuples);

    tuples
}

#[allow(dead_code)]
pub fn generate_distinct_tuples<T>(n: usize, k: usize) -> Vec<Vec<T>>
where
    T: PrimInt + SampleUniform + Hash,
{
    let mut set = HashSet::new();
    let mut rng = rng();
    let dist = Uniform::new(T::min_value(), T::max_value()).ok().unwrap();

    while set.len() < n {
        let tuple: Vec<T> = (0..k).map(|_| rng.sample(&dist)).collect();
        set.insert(tuple);
    }

    set.into_iter().collect()
}
