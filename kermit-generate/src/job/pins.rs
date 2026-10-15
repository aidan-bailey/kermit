//! What the raw IMDb cache is checked against: where `imdb.tgz` is served, the
//! digest of its bytes, the commit and the digests of the 114 files read from
//! `gregrahn/join-order-benchmark`, and the row count of each of the 21 tables
//! after loading. Nothing here is fetched or computed at run time.

/// Where `imdb.tgz` is served, in the order they are tried. Both serve the
/// same 1,263,193,115 bytes.
pub const IMDB_URLS: [&str; 2] = [
    "https://bonsai.cedardb.com/job/imdb.tgz",
    "http://event.cwi.nl/da/job/imdb.tgz",
];

/// SHA-256 (lowercase hex) of `imdb.tgz`.
///
/// `"UNMEASURED"` until Part B of the J4 task measures the dump; the download
/// in the raw cache (J7) is checked against it.
pub const IMDB_SHA256: &str = "UNMEASURED";

/// The commit of `gregrahn/join-order-benchmark` the query files are read at.
pub const QUERY_COMMIT: &str = "a39603662e023e449cb2121997a5034df9e02ebf";

/// Where the query files are served: `{QUERY_BASE_URL}/{QUERY_COMMIT}/{file}`.
pub const QUERY_BASE_URL: &str = "https://raw.githubusercontent.com/gregrahn/join-order-benchmark";

/// `schema.sql` and the 113 query files at [`QUERY_COMMIT`], each with the
/// SHA-256 (lowercase hex) of its bytes: `schema.sql` first, then `1a.sql` …
/// `33c.sql` in version order. The repository carries no licence, so kermit
/// pins the files instead of committing them.
pub const QUERY_FILES: [(&str, &str); 114] = [
    (
        "schema.sql",
        "1d10ba0c9a881e890aeb951c0960cc4e3472c9c9695ce1edb6493e9e4466d3d2",
    ),
    (
        "1a.sql",
        "53a90dac948641a2f9dd1ee12b68c66354d4c44016494d9e460b2b1879c5d7b2",
    ),
    (
        "1b.sql",
        "3f6bc4f99b49862e63aa9f0881abad66c9a4c63a96f3c6dca722d6f007854d29",
    ),
    (
        "1c.sql",
        "2111c8a7ecbdba7f4b9ce943c102e6da789df5337f1f0c90be310afe578f84bb",
    ),
    (
        "1d.sql",
        "cb8a207ee5459e05a77e9428efcdca946178c893f9c7a45dcba495427a185757",
    ),
    (
        "2a.sql",
        "71f44856563aa35d2e2612c24c80c4d67003412dbdd87fa2612a899fee6d6d9e",
    ),
    (
        "2b.sql",
        "0da8ea6095c1550ce82aff78ecb3f6d0a46a3915aede853338a77eefef529e83",
    ),
    (
        "2c.sql",
        "bf101673a7e20c7eb85795e4b0c3c55a4ae94523c2f245e9e5645476d989fbbb",
    ),
    (
        "2d.sql",
        "2d0c0b02fe1f59a4f6414461fde58befc0c51fe344ed72fef505747572354459",
    ),
    (
        "3a.sql",
        "09c04f1846b8b7d839d050b5719230e844e7612357dd749c59b37a7eb3cf9db2",
    ),
    (
        "3b.sql",
        "a36ee625b0dbf5f7a82ebd6d558d374b1753054f8361e5cb4364badc6eb05ab9",
    ),
    (
        "3c.sql",
        "ccfe237f533897b5666e7ee74ad6c8f36de19d6c0e0c537e8626b0c53791db6b",
    ),
    (
        "4a.sql",
        "c80e515f37a633adca3ab45bbd5cba20e760c80a71d7841b51e64154b1008065",
    ),
    (
        "4b.sql",
        "5e0a78655c8bf0714f365697ac2e9c4e12cc6d2096f625ff81baf33f499e6654",
    ),
    (
        "4c.sql",
        "db79ff846ccde743ab3ae806a3047eab9c9417e45ca86272fafc43bd7422ecc4",
    ),
    (
        "5a.sql",
        "b78eed60ed4341b93d663a99ef4268d2f0c589c159a3a56d32027aee85d8f17c",
    ),
    (
        "5b.sql",
        "ed0bcd17c29e612d1594693215f1a192ac954011ec2c0880a0ba7c0f159491cc",
    ),
    (
        "5c.sql",
        "7f542875e1b5247a43f7c3aad51f8fe73431866245509a98f4b321247d1794fd",
    ),
    (
        "6a.sql",
        "666385ab792d62acef9442fc2884282d3f9194a172a6c0741544ab92eaa029b2",
    ),
    (
        "6b.sql",
        "1510acb47c7b220b1328956345c9791c0d3857e06ed87a43d8fb2380c5b7d2e4",
    ),
    (
        "6c.sql",
        "4ca2c51e003a181a4099984c725328151248eed7ee708009e4d4cce2fb0cac2b",
    ),
    (
        "6d.sql",
        "8138175c4f011b8582a62921492dcf7fe4555b17fc27d4f02a9c693b519b4786",
    ),
    (
        "6e.sql",
        "03d81143246c44c4e59206d4c3960f4cfd85da1ee63d78f065c9cde1b9bc311a",
    ),
    (
        "6f.sql",
        "95279032832884404c90dcc043c61fb38f70f74d470d7598f4f00bd1532b96a0",
    ),
    (
        "7a.sql",
        "2753771bb818061fdd15aea4fc147d56a89b491a2826c0b58466e72e819ff883",
    ),
    (
        "7b.sql",
        "79b9358e5186d27007b2a3f35c18bb009b054040f85417e1858f19413993914b",
    ),
    (
        "7c.sql",
        "ba0091ee5a01f36f30910035b54bbb08582949dda268ddd4e700854e409c97bf",
    ),
    (
        "8a.sql",
        "a8d5f36074bd12f3d42934913b7847774ca70b43338beb2cde2952851f50e3f6",
    ),
    (
        "8b.sql",
        "e3cda2801dcb05a99f339b5c3ef7ed8646e1ecf382664168061029eb3ae29db6",
    ),
    (
        "8c.sql",
        "d583eba4db3a15277447be27652879cd4439ec34f1fc471982dea76729a5d099",
    ),
    (
        "8d.sql",
        "5ad3875f0bd158450b47e4a24b24e53045a6546fa0f7530a951504b61bad4e62",
    ),
    (
        "9a.sql",
        "1e2fcf63ad6fa5286704eb78c9b1c998a12605412fb34ed6f0b92923adc4f543",
    ),
    (
        "9b.sql",
        "652106b0bc0741517a54838af9e71249765856d883088bdca4ec7caf4858fd65",
    ),
    (
        "9c.sql",
        "b3a9cb8f2e5b9d07c8906793ec054e4dcbcd48ed2e8f078db87066372528664c",
    ),
    (
        "9d.sql",
        "be3b61d598df58d2b43cbabaa07997ace2e13b0c7165ae33834a2c79e295ce22",
    ),
    (
        "10a.sql",
        "dcffd7d004228d834af7bf332e365837cc5a3fb60fb3859a55235da6d1455a89",
    ),
    (
        "10b.sql",
        "66be16fbc0d5bb3a99e50581448613645e6b53286e7b15cebbb5043a98bcbb3d",
    ),
    (
        "10c.sql",
        "ccbce6a9f74d3ce0da968314672b0b5c7a479eb72621d4e845586b1150287292",
    ),
    (
        "11a.sql",
        "b9c2b7f747fd10fff3e9078534d5578ea8b53fc9dcb4bd9330952d74ec1ece0e",
    ),
    (
        "11b.sql",
        "38695698b69dbcc55ab609cdb5b13e1182127adea2b761c3ea9e2330e7a84927",
    ),
    (
        "11c.sql",
        "2d0d7040e322b74ffb48d648463a0292beb0c4d363ebddb531a8d2a0e75ea127",
    ),
    (
        "11d.sql",
        "9f3d08a09c46eb835fb3e946d1e6c22730079cfcee5dab7bf3e9a056b95a4d4e",
    ),
    (
        "12a.sql",
        "871ec385dd6e28440d15b03cf0fdd9c9376b3453bb8ec552c4e5eb1e2afdbeb8",
    ),
    (
        "12b.sql",
        "cdfa0891b60af9ef3bde65e85556a87d027557a89aa7ad6eec91366cacd6ca5f",
    ),
    (
        "12c.sql",
        "19d286824b7efb84aaa9371dbf1e842d5488e70f264a23c2c944c47f819e26c2",
    ),
    (
        "13a.sql",
        "c7d5c4fa1a5eb37bc327f531c4fe49fa4b8e2fe2ad251938880cafb3c191553d",
    ),
    (
        "13b.sql",
        "cfc30b5bc8a2d513a060f8f58c12aeb61e17526bf4ba1921de40122197341b98",
    ),
    (
        "13c.sql",
        "b61cbf84a4288c89c5494d812efed29b8a39c60dc637e37c9dedbb5fdfa2a2e0",
    ),
    (
        "13d.sql",
        "871c9d8cfa90f6299c4fb950df8c7550ee4c1fe5581c166304816c706915a8ba",
    ),
    (
        "14a.sql",
        "0c6770d9c162dca23d0d32a5d9ed2d308fc2a1b3a673463d05c81ced8445f032",
    ),
    (
        "14b.sql",
        "6878ded3d9334f2ab68c33d10ed72e912db9407babe6547469c66d2c1bb246fe",
    ),
    (
        "14c.sql",
        "cfeef4c875e3de99c575766bd17d613af1ee59e1588a2b5460e710d13bf87d7b",
    ),
    (
        "15a.sql",
        "c332055a7b8e19fcbdf3eac68f6797432ffd67d87a10267a5241d0769f0a8a0d",
    ),
    (
        "15b.sql",
        "ad3c98ba9ba0798ac5a061038d5a2a49158a196575377a21858c8b8189e551ba",
    ),
    (
        "15c.sql",
        "875e39117606a49cdb45e2ebb57261efe38ed9f786f8cae345cf620b33dd8ca1",
    ),
    (
        "15d.sql",
        "d052c7260a0fe10db83e108f25bc7a7488f82fa64d1963b9210af5799e2c36b4",
    ),
    (
        "16a.sql",
        "43cd687d2b4005c6b665762dbf7e5277135ebe738d82e672371571ca255902b9",
    ),
    (
        "16b.sql",
        "d65807d6a5dc9b37b5e484f7c57f2b469f9bd2b9e446704badb66ab29bb3a6ed",
    ),
    (
        "16c.sql",
        "85d9b65cd520f9edc7e14c0c4e39e05cc364b3f8c4ab30de07f7c343227c8edd",
    ),
    (
        "16d.sql",
        "83e7744d08279ea388a184256ab2deb8bf3c05fb93679c320fb508d10be32293",
    ),
    (
        "17a.sql",
        "a52d479958ec18149f197df005f183f8cee35712398524f344b9751c9dce9fe2",
    ),
    (
        "17b.sql",
        "da2f53d30ace326f7110861e21884165707975234072f5bdd2fff9032833f889",
    ),
    (
        "17c.sql",
        "56b241e6789c1f20b49b9cf8957154550af396b9bf6913aff974939e2f7fb7e9",
    ),
    (
        "17d.sql",
        "0dfa90692bd83e530058a329a5ee035d7bcf7d7c0d7ddba43eb6f8ba87a04601",
    ),
    (
        "17e.sql",
        "6723e52c994de2ecec2f9f0aa0cbfd9e471a15b3055905dfdabc0520ca7fe0de",
    ),
    (
        "17f.sql",
        "a03f4e3c87d842f9ac27602b01fd0c40db7f1756afb9679d40cbc3c99d553da4",
    ),
    (
        "18a.sql",
        "04816db9a582e138e9d1b2af9491520f42fa84307cf9abf3395f9c697f02d21e",
    ),
    (
        "18b.sql",
        "920582b900c1206a1eba63c935492109c108927b75bd548b7fe6229d8d879f0a",
    ),
    (
        "18c.sql",
        "3247d00269cadb278d9a417b805b928b609db6e49011f981482612434dc623a9",
    ),
    (
        "19a.sql",
        "de0e1cae5dd400d5fc0b2954eaa41616861e76e1f4a45bbf923a8b94660fee53",
    ),
    (
        "19b.sql",
        "2148cf91cbf25063f906086d1944a00f73d5b07e5076a9f1ed8bb4712fb12807",
    ),
    (
        "19c.sql",
        "fe5b602cc75a95ba14f4694bfbe5d8082c405aa0881a5e3e2c259ed5e0f9e8df",
    ),
    (
        "19d.sql",
        "f450bd39710978c8bfbf961587cb3649190effe724855e6345bcf86d42ad39dc",
    ),
    (
        "20a.sql",
        "eea79cc4c815789c64bf7f4306e8e30db85289a8938bbcd012cb22388264067f",
    ),
    (
        "20b.sql",
        "36c7ddba787747bca16a6dcb63ad50fba60f766afe771e5d3555b71607b98298",
    ),
    (
        "20c.sql",
        "86f02702929dee0430d2ba6e447d3454f518a09a0b9f5504dacf80a5753c5e89",
    ),
    (
        "21a.sql",
        "c899a4175c318b17df6c253c53253a130efeed6e6d2230180b02e08e9dcba722",
    ),
    (
        "21b.sql",
        "33f2c3edf99b23a99ea819fb41a0ff63af19f3ffbcd6e98ed94d1decb1accb67",
    ),
    (
        "21c.sql",
        "e62c4d11928d31a55a09fc2aef9bc3812e510f016ecc2463596a099a1c4a9366",
    ),
    (
        "22a.sql",
        "eec5b2d12dc15e51fb6bf5e83d13c0bc185ca150b4d75c07bda8eb57f861a29c",
    ),
    (
        "22b.sql",
        "819cce32cbcac10161aaeab26817e0136e0370bb4b9cbca5cc415172bfd0ff2c",
    ),
    (
        "22c.sql",
        "9401e4b3000f15cb9fd1a39abfda016c8f94f079c9cd8acc93d314a23e8890c5",
    ),
    (
        "22d.sql",
        "96691d9b9a24284e9fa6992ab4c83b285579a1f1a5cf7cf4bd943b6c1fc91a71",
    ),
    (
        "23a.sql",
        "0115941960633919edac315eb3e40d1df1953a66b93cbab316244018d2adcb09",
    ),
    (
        "23b.sql",
        "893cb0185455fb67072b934bf12158b9dc217bc9e3ffe680deabf84c53fbc713",
    ),
    (
        "23c.sql",
        "93566f0155c4f6816b51eb71d52aecaec868a1b160c38b4643e5cb9780bc9c3d",
    ),
    (
        "24a.sql",
        "2364e4924f1ad3a81b07420e6d5310d3220df7474c5b11b50ed062e884daf480",
    ),
    (
        "24b.sql",
        "199711fb4c5f118e67fbd926218b66cb96a8c2530668bc43fbe812af2a658314",
    ),
    (
        "25a.sql",
        "643c0557c8d898467d56c492ff7b0ebe4ebacaa6131af16e0b22f26ba9f21ace",
    ),
    (
        "25b.sql",
        "aad58fb332fd9c7c8a67067086dbc32b7884e252435bf17d3c0fbcd69d5cbae1",
    ),
    (
        "25c.sql",
        "4a33e40c4c643d39a8d5933a4d2960a8a3f2a100e5a063ac2c20f8b007d8e5f8",
    ),
    (
        "26a.sql",
        "e68db6c6477fcd41adde48510c3eaec1765ad13fa9e6b899348dc996f6a589cc",
    ),
    (
        "26b.sql",
        "b0393215e99a169caa77ba893beed022e1090c9044cc412c304a31b24370f242",
    ),
    (
        "26c.sql",
        "afd4656f06cdad04640e6ccf4db24cc251954689da270e9de259c89825d50404",
    ),
    (
        "27a.sql",
        "b470129ed757160c21317ecd59d0fd71a3aac72052c2472a43f3007170cd6d0d",
    ),
    (
        "27b.sql",
        "5e807f3f664f140b3bbe7637036c8bd85183ad9343b9e18d4dde49a4b30751d6",
    ),
    (
        "27c.sql",
        "1d00d9231b2b3d9e379c5e0788fdb38e49c42bc2a73398d9588385022105cdac",
    ),
    (
        "28a.sql",
        "f4d032f672de59f119453a2f948717e1c88dbd2358e1fb5186ca295030eac9d1",
    ),
    (
        "28b.sql",
        "a0f8a1130ecd9954c3acb5058e91d436edeab93bff589823d8cb58dae83ae14c",
    ),
    (
        "28c.sql",
        "8a5d2826e0d8edada1aa3afb1266e4988fc17ffa5cd0ccfa5b3410258958d7b5",
    ),
    (
        "29a.sql",
        "a693c52de057655e421c2ab5569503ac54da257653dcb0cde8020eef027b5c96",
    ),
    (
        "29b.sql",
        "a43ad7b45d9e01c7983b565c229356722e3f7028faecee48d66b5d7546181772",
    ),
    (
        "29c.sql",
        "b338eb16c2ff5a5022ef609b9a77e6c4ef76a9771eca8de5785aef382ea33078",
    ),
    (
        "30a.sql",
        "9ec9bb8b2521099af76e45201b970cc4373478ad8c0eb0323a4a02068751c802",
    ),
    (
        "30b.sql",
        "6294013729f3373603d5dd24e49df10a3f87f51c883054d19e7112c493a91e3f",
    ),
    (
        "30c.sql",
        "1fc2c1125858123d908e2808f46471a76c0e1e787161e88e32670c15b1da6d8e",
    ),
    (
        "31a.sql",
        "dd779ae7e005300c9af6fd006ab07ff93d75cdd766df13467b73955271a6e02b",
    ),
    (
        "31b.sql",
        "c97f5c77bede5ce118782f92c87a3dd05fd93d2453749860124ba19c77451d51",
    ),
    (
        "31c.sql",
        "8064d891a412b1d953c11b8b8c60d5666c3e017aa3048dc29e1648e393d984b3",
    ),
    (
        "32a.sql",
        "920bf48d8f22cb80a07f4d6d1567614fc6ca269487e18e4a897c72443529427b",
    ),
    (
        "32b.sql",
        "b8bb4489b603250702299fe3a8eb763a514f2fa42568524a2d486f692b7d0eb7",
    ),
    (
        "33a.sql",
        "16ec76ddb7fed9155cf98d85c3a6741cc75c1f08e45da9ffda3206a0d8215c2b",
    ),
    (
        "33b.sql",
        "89639c0c29423a3fe0d8bc4ddf3314ca83c4be72a09ec201c6cb14df0604ca5c",
    ),
    (
        "33c.sql",
        "ad00e304413df75daefbde27f18872bcfc33f1bcc86a95c6dd133d4a5fc27f1d",
    ),
];

/// The row count of each of the 21 IMDb tables after loading `imdb.tgz`,
/// in `schema.sql`'s order.
///
/// All `0` until Part B of the J4 task measures them; the raw-cache build
/// checks every loaded table against its entry.
pub const IMDB_TABLE_ROWS: [(&str, u64); 21] = [
    ("aka_name", 0),
    ("aka_title", 0),
    ("cast_info", 0),
    ("char_name", 0),
    ("comp_cast_type", 0),
    ("company_name", 0),
    ("company_type", 0),
    ("complete_cast", 0),
    ("info_type", 0),
    ("keyword", 0),
    ("kind_type", 0),
    ("link_type", 0),
    ("movie_companies", 0),
    ("movie_info", 0),
    ("movie_info_idx", 0),
    ("movie_keyword", 0),
    ("movie_link", 0),
    ("name", 0),
    ("person_info", 0),
    ("role_type", 0),
    ("title", 0),
];

/// The URL of one query file (or `schema.sql`) at [`QUERY_COMMIT`].
pub fn query_url(file: &str) -> String { format!("{QUERY_BASE_URL}/{QUERY_COMMIT}/{file}") }

#[cfg(test)]
mod tests {
    use {super::*, std::collections::HashSet};

    /// The 21 tables `schema.sql` creates, in its order.
    const SCHEMA_TABLES: [&str; 21] = [
        "aka_name",
        "aka_title",
        "cast_info",
        "char_name",
        "comp_cast_type",
        "company_name",
        "company_type",
        "complete_cast",
        "info_type",
        "keyword",
        "kind_type",
        "link_type",
        "movie_companies",
        "movie_info",
        "movie_info_idx",
        "movie_keyword",
        "movie_link",
        "name",
        "person_info",
        "role_type",
        "title",
    ];

    #[test]
    fn query_files_are_unique_and_hex() {
        let mut seen = HashSet::new();
        for (file, digest) in QUERY_FILES {
            assert!(seen.insert(file), "{file} is pinned twice");
            assert_eq!(digest.len(), 64, "{file}");
            assert!(
                digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{file}: {digest} is not lowercase hex"
            );
        }
    }

    /// `schema.sql` first, then the 113 queries of the 33 templates in version
    /// order (`9d` before `10a`), which is the order the files are fetched in.
    #[test]
    fn query_files_are_schema_then_113_queries_in_version_order() {
        assert_eq!(QUERY_FILES[0].0, "schema.sql");
        assert_eq!(QUERY_FILES.len() - 1, 113);
        let mut previous = (0u32, String::new());
        let mut templates = HashSet::new();
        for (file, _) in &QUERY_FILES[1..] {
            let stem = file.strip_suffix(".sql").unwrap();
            let split = stem.find(|c: char| !c.is_ascii_digit()).unwrap();
            let (number, letters) = stem.split_at(split);
            let key = (number.parse::<u32>().unwrap(), letters.to_owned());
            assert!((1..=33).contains(&key.0), "{file}");
            assert!(key > previous, "{file} is out of order");
            templates.insert(key.0);
            previous = key;
        }
        assert_eq!(templates.len(), 33);
    }

    #[test]
    fn query_url_has_the_commit() {
        assert_eq!(
            query_url("6a.sql"),
            "https://raw.githubusercontent.com/gregrahn/join-order-benchmark/\
             a39603662e023e449cb2121997a5034df9e02ebf/6a.sql"
        );
        assert!(query_url("schema.sql").contains(QUERY_COMMIT));
    }

    #[test]
    fn imdb_table_rows_name_the_21_tables() {
        let names: Vec<&str> = IMDB_TABLE_ROWS.iter().map(|(table, _)| *table).collect();
        assert_eq!(names, SCHEMA_TABLES);
    }
}
