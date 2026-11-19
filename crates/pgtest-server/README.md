# Postgres test server

Equivalent command for manual usage:

```bash

podman run --rm --detach --name pgtest \
    --image-volume tmpfs \
    --publish 127.0.0.1:55432:5432 \
    --env POSTGRES_USER=postgres \
    --env POSTGRES_PASSWORD=postgres \
    --env POSTGRES_DB=postgres \
    docker.io/library/postgres:18

```

Then: `psql -h 127.0.0.1 -p 55432 -U postgres`