
CREATE TABLE "indexer_sync" (
  doc_id VARCHAR PRIMARY KEY NOT NULL,
  indexed_clock TIMESTAMP NOT NULL DEFAULT 0,
  indexer_version INTEGER NOT NULL DEFAULT 0
);