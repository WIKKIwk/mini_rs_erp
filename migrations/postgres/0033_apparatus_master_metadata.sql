-- Persist semantic apparatus metadata alongside the stable apparatus catalog id.
-- Existing sort_order and other payload keys are intentionally preserved.

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"pechat","kind":"color_pechat","capabilities":["print","pechat"],"color_stations":7}'::jsonb
WHERE id = 'apparatus:default:bosma_7';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"pechat","kind":"color_pechat","capabilities":["print","pechat"],"color_stations":8}'::jsonb
WHERE id = 'apparatus:default:bosma_8';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"pechat","kind":"color_pechat","capabilities":["print","pechat"],"color_stations":9}'::jsonb
WHERE id = 'apparatus:default:bosma_9';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"laminatsiya","kind":"extruder_laminatsiya","capabilities":["laminate"]}'::jsonb
WHERE id = 'apparatus:default:extruder_laminatsiya';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"pechat","kind":"flexo","capabilities":["print","pechat","flexo"]}'::jsonb
WHERE id = 'apparatus:default:flexo_pechat';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"kley","kind":"holodniy_kley","capabilities":["glue"]}'::jsonb
WHERE id = 'apparatus:default:holodniy_kley';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"laminatsiya","kind":"laminatsiya","capabilities":["laminate"]}'::jsonb
WHERE id IN ('apparatus:default:laminatsiya_1', 'apparatus:default:laminatsiya_2');

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"paket","kind":"paket","capabilities":["package"]}'::jsonb
WHERE id = 'apparatus:default:paket';

UPDATE mini_apparatus
SET payload_json = payload_json || '{"family":"rezka","kind":"rezka","capabilities":["cut"]}'::jsonb
WHERE id = 'apparatus:default:rezka';
