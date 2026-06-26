# Fedora Deploy: Pingora Gateway + Native PostgreSQL

This runbook moves the Fedora Mini RS ERP host from Cloudflare Tunnel + Docker
PostgreSQL toward a direct public origin:

```text
Cloudflare DNS/proxy -> Fedora :443 -> mini_rs_gateway -> mini_rs_erp :18081 -> PostgreSQL :5432
```

Do not remove the old Cloudflare Tunnel or Docker database until all verification
steps pass.

## Paths

```bash
APP_ROOT=/home/wikki/mini_rs_erp_deploy
CURRENT=$APP_ROOT/current
ENV_FILE=$APP_ROOT/.env
```

## 1. Backup Current Docker PostgreSQL

```bash
TS=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR=/home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS
mkdir -p "$BACKUP_DIR"

docker exec mini-rs-erp-postgres pg_dump -U mini_rs_erp -d mini_rs_erp \
  | gzip > "$BACKUP_DIR/mini_rs_erp.sql.gz"

if [ -f /home/wikki/mini_rs_erp_deploy/.env ]; then
  grep '^MINI_ERP_DATABASE_URL=' /home/wikki/mini_rs_erp_deploy/.env \
    > "$BACKUP_DIR/database_url.env" || true
fi
```

## 2. Install Native PostgreSQL

```bash
sudo dnf install -y postgresql-server postgresql-contrib

if [ ! -d /var/lib/pgsql/data/base ]; then
  sudo postgresql-setup --initdb
fi

sudo systemctl enable --now postgresql
sudo systemctl status postgresql --no-pager
```

PostgreSQL must listen only on localhost for this architecture.

```bash
sudo -u postgres psql -tAc "show listen_addresses;"
```

Expected safe value:

```text
localhost
```

## 3. Create Database And User

```bash
read -r -s MINI_RS_ERP_DB_PASSWORD
export MINI_RS_ERP_DB_PASSWORD

sudo -u postgres psql <<'SQL'
DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
    CREATE ROLE mini_rs_erp LOGIN;
  END IF;
END $$;
SQL

sudo -u postgres psql -v password="$MINI_RS_ERP_DB_PASSWORD" <<'SQL'
ALTER ROLE mini_rs_erp WITH PASSWORD :'password';
SELECT 'CREATE DATABASE mini_rs_erp OWNER mini_rs_erp'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mini_rs_erp')
\gexec
SQL
```

## 4. Restore Backup Into Native PostgreSQL

```bash
gunzip -c "$BACKUP_DIR/mini_rs_erp.sql.gz" \
  | sudo -u postgres psql -d mini_rs_erp
```

Verify core tables before starting the service:

```bash
sudo -u postgres psql -d mini_rs_erp <<'SQL'
SELECT 'mini_orders' AS table_name, count(*) FROM mini_orders
UNION ALL
SELECT 'mini_items', count(*) FROM mini_items
UNION ALL
SELECT 'mini_item_groups', count(*) FROM mini_item_groups
UNION ALL
SELECT 'production_maps', count(*) FROM production_maps;
SQL
```

## 5. Write Runtime Env

Start from the repository template:

```bash
mkdir -p /home/wikki/mini_rs_erp_deploy/data
cp deploy/systemd/mini-rs-erp-native-postgres.env.example /tmp/mini-rs-env
perl -0pi -e "s#postgres://mini_rs_erp:<password>\\@127\\.0\\.0\\.1:5432/mini_rs_erp#postgres://mini_rs_erp:$ENV{MINI_RS_ERP_DB_PASSWORD}\\@127.0.0.1:5432/mini_rs_erp#g" /tmp/mini-rs-env
install -m 600 /tmp/mini-rs-env /home/wikki/mini_rs_erp_deploy/.env
```

## 6. Install Services

Backend service must bind to localhost:

```ini
[Unit]
Description=Mini RS ERP backend
After=network-online.target postgresql.service
Wants=network-online.target
Requires=postgresql.service

[Service]
Type=simple
User=wikki
Group=wikki
WorkingDirectory=/home/wikki/mini_rs_erp_deploy/current
EnvironmentFile=/home/wikki/mini_rs_erp_deploy/.env
ExecStart=/home/wikki/mini_rs_erp_deploy/current/mini_rs_erp
Restart=always
RestartSec=3
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
```

Gateway service is stored in the repo:

```bash
sudo cp deploy/systemd/mini-rs-gateway.service /etc/systemd/system/mini-rs-gateway.service
sudo systemctl daemon-reload
sudo systemctl enable mini-rs-erp mini-rs-gateway
sudo systemctl restart mini-rs-erp
sudo systemctl restart mini-rs-gateway
```

## 7. Local Verification On Fedora

```bash
curl -fsS http://127.0.0.1:18081/healthz
curl -kfsS https://127.0.0.1/healthz
systemctl status mini-rs-erp --no-pager
systemctl status mini-rs-gateway --no-pager
```

Expected backend health response:

```json
{"ok":true}
```

Expected gateway health response:

```json
{"ok":true,"service":"mini_rs_gateway"}
```

## 8. DNS And Firewall Cutover

Open only HTTPS to the public internet:

```bash
sudo firewall-cmd --add-service=https --permanent
sudo firewall-cmd --reload
sudo firewall-cmd --list-services
```

Cloudflare DNS for `mini-rs-erp-test.wspace.sbs` must point to Fedora's public
IP and remain proxied.

## 9. Public Verification

```bash
for i in 1 2 3 4 5; do
  curl -fsS -o /dev/null \
    -w "domain_$i http=%{http_code} total=%{time_total}\n" \
    https://mini-rs-erp-test.wspace.sbs/healthz
done
```

WebSocket live endpoints must stay connected through the domain:

```bash
node /tmp/mini-rs-ws-1k.js
```

## 10. Rollback

```bash
sudo systemctl stop mini-rs-gateway
sudo systemctl restart cloudflared-mini-rs-erp.service

if [ -f "$BACKUP_DIR/database_url.env" ]; then
  grep -v '^MINI_ERP_DATABASE_URL=' /home/wikki/mini_rs_erp_deploy/.env > /tmp/mini-rs-env
  cat "$BACKUP_DIR/database_url.env" >> /tmp/mini-rs-env
  install -m 600 /tmp/mini-rs-env /home/wikki/mini_rs_erp_deploy/.env
  sudo systemctl restart mini-rs-erp
fi
```

Rollback is complete only after:

```bash
curl -fsS https://mini-rs-erp-test.wspace.sbs/healthz
```
