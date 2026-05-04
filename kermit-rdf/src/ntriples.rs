//! Streaming N-Triples parser.
//!
//! Wraps `oxttl::NTriplesParser` and yields `(subject, predicate, object)`
//! triples whose subject and predicate must be IRIs (the only forms WatDiv
//! emits). Blank-node subjects, literal predicates, etc. are surfaced as
//! parse errors. Iteration is line-streaming: memory usage is O(1) in the
//! file size.

use {
    crate::{error::RdfError, value::RdfValue},
    oxttl::NTriplesParser,
    std::{io::Read, path::Path},
};

/// Iterator yielding parsed triples from an N-Triples source.
pub struct TripleIter<R: Read> {
    inner: oxttl::ntriples::ReaderNTriplesParser<R>,
    line: usize,
}

impl<R: Read> Iterator for TripleIter<R> {
    type Item = Result<(String, String, RdfValue), RdfError>;

    fn next(&mut self) -> Option<Self::Item> {
        let triple = self.inner.next()?;
        self.line += 1;
        Some(map_triple(triple, self.line))
    }
}

fn map_triple(
    parsed: Result<oxrdf::Triple, oxttl::TurtleParseError>, line: usize,
) -> Result<(String, String, RdfValue), RdfError> {
    let triple = parsed.map_err(|e| RdfError::NTriplesParse {
        line,
        message: e.to_string(),
    })?;
    let subject_iri = match triple.subject {
        | oxrdf::Subject::NamedNode(n) => n.into_string(),
        | other => {
            return Err(RdfError::NTriplesParse {
                line,
                message: format!("non-IRI subject: {other}"),
            });
        },
    };
    let predicate_iri = triple.predicate.into_string();
    let object = match triple.object {
        | oxrdf::Term::NamedNode(n) => RdfValue::Iri(n.into_string()),
        | oxrdf::Term::BlankNode(b) => RdfValue::BlankNode(b.to_string()),
        | oxrdf::Term::Literal(l) => RdfValue::Literal(l.to_string()),
    };
    Ok((subject_iri, predicate_iri, object))
}

/// Returns an iterator over the triples of an N-Triples file.
pub fn iter_path<P: AsRef<Path>>(
    path: P,
) -> Result<TripleIter<std::io::BufReader<std::fs::File>>, RdfError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let inner = NTriplesParser::new().for_reader(reader);
    Ok(TripleIter {
        inner,
        line: 0,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, std::io::Write};

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_iri_object() {
        let f = write_temp("<http://x> <http://p> <http://y> .\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert_eq!(triples.len(), 1);
        let (s, p, o) = triples[0].as_ref().unwrap();
        assert_eq!(s, "http://x");
        assert_eq!(p, "http://p");
        assert_eq!(*o, RdfValue::Iri("http://y".to_string()));
    }

    #[test]
    fn parses_literal_object() {
        let f = write_temp("<http://x> <http://p> \"hello\" .\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        let (_, _, o) = triples[0].as_ref().unwrap();
        assert!(matches!(o, RdfValue::Literal(_)));
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let f = write_temp("# header\n\n<http://x> <http://p> <http://y> .\n# footer\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert_eq!(triples.len(), 1);
    }

    #[test]
    fn malformed_line_errors() {
        let f = write_temp("<not a valid triple>\n");
        let triples: Vec<_> = iter_path(f.path()).unwrap().collect();
        assert!(triples[0].is_err());
    }
}
