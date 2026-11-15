//! `Reader::list`, `Promoter::promote`, `Maintainer::plan` and `MemoryIndex::from_reader` can fail, so a
//! key-value-backed store can implement `Reader`.

use sageant::{
    Chunk, ChunkId, ChunkRef, Confidence, Error, ExpireMaintainer, Filter, Hit, Link, Maintainer,
    Plan, Promoter, Reader, Timestamp,
};

/// A reader whose every listing fails.
struct Broken;
impl Reader for Broken {
    fn get(&self, _: &ChunkId) -> Result<Chunk, Error> {
        Err(Error::Backend("down".into()))
    }
    fn get_version(&self, _: &ChunkId, _: u32) -> Result<Chunk, Error> {
        Err(Error::Backend("down".into()))
    }
    fn history(&self, _: &ChunkId) -> Result<Vec<Chunk>, Error> {
        Err(Error::Backend("down".into()))
    }
    fn links_from(&self, _: &ChunkId) -> Result<Vec<Link>, Error> {
        Err(Error::Backend("down".into()))
    }
    fn links_to(&self, _: &ChunkId) -> Result<Vec<Link>, Error> {
        Err(Error::Backend("down".into()))
    }
    fn confidence(&self, _: &ChunkId) -> Result<Confidence, Error> {
        Err(Error::Backend("down".into()))
    }
    fn list(&self, _: &Filter) -> Result<Vec<ChunkRef>, Error> {
        Err(Error::Backend("down".into()))
    }
}

#[test]
fn list_reports_a_backend_failure() {
    assert!(matches!(
        Broken.list(&Filter::new()),
        Err(Error::Backend(_))
    ));
}

#[test]
fn a_maintainer_reports_a_failed_listing() {
    let r: Result<Plan, Error> = ExpireMaintainer::new(Timestamp(50)).plan(&Broken);
    assert!(matches!(r, Err(Error::Backend(_))));
}

struct Recent;
impl Promoter for Recent {
    fn promote(&self, _: &[u8], reader: &dyn Reader) -> Result<Vec<Hit>, Error> {
        reader.list(&Filter::new()).map(|_| Vec::new())
    }
}

#[test]
fn a_promoter_reports_a_failed_listing() {
    assert!(Recent.promote(b"", &Broken).is_err());
}
