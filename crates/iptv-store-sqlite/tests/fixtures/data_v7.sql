-- Synthetic persisted library settings in v7.
INSERT INTO source_policy(source_id,enabled,refresh_seconds,refresh_on_start) VALUES (1,0,21600,1);
INSERT INTO group_overlays(source_id,group_id,custom_name,hidden,position) VALUES
 (1,'xt:live-group:2','Local sports',1,0),
 (1,'xt:live-group:1',NULL,0,1);
