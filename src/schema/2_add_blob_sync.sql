
CREATE TABLE "peer_blob_sync" (
  peer VARCHAR NOT NULL,
  blob_id VARCHAR NOT NULL,
  uploaded_at TIMESTAMP,
  PRIMARY KEY (peer, blob_id)
);
CREATE INDEX peer_blob_sync_peer ON peer_blob_sync (peer);