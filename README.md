<div align="center">

<h2>DuckWatch</h2>

[![Tests](https://img.shields.io/github/actions/workflow/status/CogitatorTech/duck-watch/tests.yml?label=tests&style=flat&labelColor=282c34&logo=github)](https://github.com/CogitatorTech/duck-watch/actions/workflows/tests.yml)
[![Code Coverage](https://img.shields.io/codecov/c/github/CogitatorTech/duck-watch?label=coverage&style=flat&labelColor=282c34&logo=codecov)](https://codecov.io/gh/CogitatorTech/duck-watch)
[![License](https://img.shields.io/badge/license-Apache--2.0-007ec6?style=flat&labelColor=282c34&logo=open-source-initiative)](LICENSE)
[![Container Images](https://img.shields.io/github/v/tag/CogitatorTech/duck-watch?label=ghcr.io&style=flat&labelColor=282c34&logo=docker&color=507ec6&sort=semver)](https://github.com/CogitatorTech/duck-watch/pkgs/container/duck-watch-backend)

A minimalist observability tool for MotherDuck

</div>

---

DuckWatch shows you what your MotherDuck account is doing and what it is costing you.

### Key Features

- Latency, with percentiles, slow query lists, and a chart over time
- Failures, with error types and a failures-only view
- Cost estimates, attributed per query, per user, per Duckling size, and per query shape, priced from MotherDuck's published rates, plus a monthly run rate for storage
- Insights about queries (including anti-pattern findings such as select *, cross joins, missing filters, and spilling, grouped by query shape (like queries that differ only in their literals))


<br>
<div align="center">
  <img alt="Stats" src="docs/assets/screenshots/2.png" width="80%">
</div>

<details>
<summary>Show more screenshots</summary>

<div align="center">
  <img alt="Shot 1" src="docs/assets/screenshots/1.png" width="80%">
  <img alt="Shot 2" src="docs/assets/screenshots/4.png" width="80%">
  <img alt="Shot 3" src="docs/assets/screenshots/3.png" width="80%">
  <img alt="Shot 4" src="docs/assets/screenshots/5.png" width="80%">
</div>

</details>

---

### Getting Started

> [!IMPORTANT]
> To run and use DuckWatch as described here, you need to have Docker installed on your machine.
> Additionally, you need a MotherDuck account on the Business or Enterprise plan, and a read-only service token
> created under a role with the view query history permission.
> The Admin and Builder preset roles include that permission by default.
> The Lite plan does not include query history, so DuckWatch cannot read anything on it.

##### 1. Create a `docker-compose.yml` File

Save the text below as `docker-compose.yml` in an empty directory.

```yaml
services:
    db:
        image: postgres:18-alpine
        environment:
            POSTGRES_PASSWORD: postgres
        volumes:
            - duckwatch-db:/var/lib/postgresql
        healthcheck:
            test: [ "CMD-SHELL", "pg_isready -U postgres" ]
            interval: 5s
            timeout: 5s
            retries: 5
        restart: unless-stopped

    backend:
        image: ghcr.io/cogitatortech/duck-watch-backend:latest
        environment:
            DATABASE_URL: postgres://postgres:postgres@db:5432/postgres?sslmode=disable
            TOKEN_ENCRYPTION_KEY: ${TOKEN_ENCRYPTION_KEY:?run the key command below first}
        depends_on:
            db:
                condition: service_healthy
        restart: unless-stopped

    web:
        image: ghcr.io/cogitatortech/duck-watch-web:latest
        ports:
            - "3000:80"
        depends_on:
            - backend
        restart: unless-stopped

volumes:
    duckwatch-db:
```

##### 2. Encryption Key 

Generate and write an encryption key into a `.env` file beside `docker-compose.yml`.

```sh
printf 'TOKEN_ENCRYPTION_KEY="%s"\n' "$(openssl rand -base64 32)" > .env
```

##### 3. Starting DuckWatch

Run `docker compose up -d` in the directory where `docker-compose.yml` is, and open http://localhost:3000 in your browser.

#### Managing DuckWatch Containers

You can use docker compose commands to manage the DuckWatch containers:

```sh
docker compose up -d      # Start DuckWatch
docker compose stop       # Stop DuckWatch (data is kept)
docker compose down -v    # Remove everything, including the database
docker compose logs -f    # Check the log stream
```

---

### Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for details on how to make a contribution.

### License

DuckWatch is licensed under the Apache License, Version 2.0 (see [LICENSE](LICENSE)).
