<div align="center">
  <picture>
    <img alt="DuckWatch Logo" src="logo.svg" width="120" height="120">
  </picture>
<br>

<h2>DuckWatch</h2>

[![Tests](https://img.shields.io/github/actions/workflow/status/CogitatorTech/duck-watch/tests.yml?label=tests&style=flat&labelColor=282c34&logo=github)](https://github.com/CogitatorTech/duck-watch/actions/workflows/tests.yml)
[![Code Coverage](https://img.shields.io/codecov/c/github/CogitatorTech/duck-watch?label=coverage&style=flat&labelColor=282c34&logo=codecov)](https://codecov.io/gh/CogitatorTech/duck-watch)
[![License](https://img.shields.io/badge/license-Apache--2.0-007ec6?style=flat&labelColor=282c34&logo=open-source-initiative)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-ghcr.io-007ec6?style=flat&labelColor=282c34&logo=docker)](https://github.com/CogitatorTech/duck-watch/pkgs/container/duck-watch-backend)

An observability tool for MotherDuck

</div>

---

DuckWatch shows you what your MotherDuck account is doing and what it is costing you. That includes

---

### Getting Started

**1. Save the text below as `docker-compose.yml` in an empty directory.**

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

**2. Generate and write an encryption key into a `.env` file beside `docker-compose.yml`.**

```sh
printf 'TOKEN_ENCRYPTION_KEY="%s"\n' "$(openssl rand -base64 32)" > .env
```

**3. Run `docker compose up -d` in the directory where `docker-compose.yml` is, and open http://localhost:3000 in your browser.**

#### DuckWatch Containers

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
