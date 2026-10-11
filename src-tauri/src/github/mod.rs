mod graphql;
pub mod rest_writes;

pub use graphql::{
    fetch_all_orgs, fetch_all_orgs_multi, fetch_org_open_items, FetchResult, GraphqlError,
    MultiOrgFetchResult,
};
