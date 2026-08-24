SHELL := /bin/bash
BACKEND_DIR := backend
WEB_DIR := web
COMPOSE := $(shell docker compose version >/dev/null 2>&1 && echo "docker compose" || echo "docker-compose")
RUST_BACKTRACE := 1

# Integration tests and manual migrations need this to match the `db` service in docker-compose.yaml.
DATABASE_URL ?= postgres://postgres:postgres@localhost:5432/postgres?sslmode=disable
export DATABASE_URL
export RUST_BACKTRACE

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show help messages for all available targets
	@grep -E '^[a-zA-Z_-]+:.*## .*$$|^##@ ' Makefile | \
	awk 'BEGIN {FS = ":.*## "}; \
	     /^##@ / {printf "\n\033[1m%s\033[0m\n", substr($$0, 5); next}; \
	     {printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'

##@ Backend

.PHONY: format
format: ## Format the Rust code
	@echo "Formatting Rust files..."
	@cargo fmt

.PHONY: lint
lint: format ## Run clippy over the workspace
	@echo "Linting Rust files..."
	@cargo clippy --all-targets --workspace -- -D warnings -D clippy::unwrap_used -D clippy::expect_used

.PHONY: fix-lint
fix-lint: ## Apply the clippy suggestions that can be fixed automatically
	@echo "Fixing linter warnings..."
	@cargo clippy --fix --allow-dirty --allow-staged --all-targets --workspace -- -D warnings

.PHONY: build
build: ## Build the backend in release mode
	@echo "Building the backend..."
	@cargo build --release

.PHONY: test
test: ## Run the unit tests (no containers needed)
	@echo "Running unit tests..."
	@cargo test --workspace

.PHONY: test-integration
test-integration: ## Run the unit and integration tests (needs `make docker-up`)
	@echo "Running unit and integration tests..."
	@cargo test --workspace --features integration-tests

.PHONY: run-backend
run-backend: ## Run the backend locally (needs `make docker-up`)
	@if [ ! -f $(BACKEND_DIR)/.env ]; then \
	   cp $(BACKEND_DIR)/.env.example $(BACKEND_DIR)/.env; \
	   key=$$(openssl rand -base64 32 2>/dev/null || head -c 32 /dev/urandom | base64); \
	   sed -i.bak "s|^TOKEN_ENCRYPTION_KEY=.*|TOKEN_ENCRYPTION_KEY=\"$$key\"|" $(BACKEND_DIR)/.env; \
	   rm -f $(BACKEND_DIR)/.env.bak; \
	   echo "Created $(BACKEND_DIR)/.env with a freshly generated TOKEN_ENCRYPTION_KEY."; \
	fi
	@cd $(BACKEND_DIR) && cargo run

.PHONY: coverage
coverage: ## Write an lcov coverage report for the backend (needs `make docker-up`)
	@echo "Measuring backend coverage..."
	@cargo llvm-cov --workspace --features integration-tests --lcov --output-path lcov.info

.PHONY: docs
docs: ## Generate the Rust API documentation
	@echo "Generating documentation..."
	@cargo doc --no-deps --document-private-items

.PHONY: audit
audit: ## Check the Rust dependencies for known advisories
	@echo "Running security audit..."
	@cargo audit

##@ Frontend

.PHONY: install-web
install-web: ## Install the frontend dependencies
	@cd $(WEB_DIR) && npm install

.PHONY: format-web
format-web: ## Format the frontend code
	@cd $(WEB_DIR) && npm run format

.PHONY: lint-web
lint-web: ## Check the frontend formatting and lint rules
	@cd $(WEB_DIR) && npm run lint

.PHONY: check-web
check-web: ## Type check the frontend
	@cd $(WEB_DIR) && npm run check

.PHONY: test-web
test-web: ## Run the frontend tests
	@cd $(WEB_DIR) && npm test

.PHONY: build-web
build-web: ## Build the frontend for production
	@cd $(WEB_DIR) && npm run build

.PHONY: run-web
run-web: ## Run the frontend development server
	@if [ ! -f $(WEB_DIR)/.env ]; then \
	   cp $(WEB_DIR)/.env.example $(WEB_DIR)/.env; \
	   echo "Created $(WEB_DIR)/.env from the example file."; \
	fi
	@cd $(WEB_DIR) && npm run dev

##@ Services and Database

.PHONY: setup
setup: ## Create the .env file that Docker Compose reads (run once)
	@if [ ! -f .env ]; then \
	   key=$$(openssl rand -base64 32 2>/dev/null || head -c 32 /dev/urandom | base64); \
	   printf 'TOKEN_ENCRYPTION_KEY="%s"\n' "$$key" > .env; \
	   echo "Created .env with a freshly generated TOKEN_ENCRYPTION_KEY."; \
	   echo "Back it up with your database: losing it orphans every stored token."; \
	else \
	   echo ".env already exists, so the existing key was kept."; \
	fi

.PHONY: start
start: setup ## Run DuckWatch with Docker, on http://localhost:3000
	@$(COMPOSE) up -d --pull always
	@echo ""
	@echo "DuckWatch is starting on http://localhost:3000"
	@echo "Follow it with 'make logs'."

.PHONY: start-dev
start-dev: setup ## Run DuckWatch, building both images from this working tree
	@$(COMPOSE) -f docker-compose.yaml -f docker-compose.dev.yaml up -d --build
	@echo ""
	@echo "DuckWatch is starting on http://localhost:3000"
	@echo "Building the backend takes about twenty minutes the first time: the"
	@echo "duckdb crate compiles libduckdb from C++. Follow it with 'make logs'."

.PHONY: stop
stop: ## Stop DuckWatch, keeping its data
	@$(COMPOSE) stop

.PHONY: logs
logs: ## Follow the container logs
	@$(COMPOSE) logs -f

.PHONY: docker-up
docker-up: ## Start the PostgreSQL container
	@echo "Starting the database container..."
	@$(COMPOSE) up -d db

.PHONY: docker-down
docker-down: ## Stop the containers and remove their volumes
	@echo "Stopping the containers..."
	@$(COMPOSE) down -v

.PHONY: docker-build
docker-build: ## Build the backend and frontend images
	@echo "Building the images..."
	@$(COMPOSE) build

.PHONY: docker-run
docker-run: ## Run the whole stack in containers
	@echo "Starting the stack..."
	@$(COMPOSE) up -d --build

.PHONY: db-migrate
db-migrate: ## Apply the pending migrations (needs sqlx-cli)
	@echo "Applying migrations..."
	@cd $(BACKEND_DIR) && sqlx migrate run

.PHONY: db-revert
db-revert: ## Revert the most recent migration (needs sqlx-cli)
	@echo "Reverting the last migration..."
	@cd $(BACKEND_DIR) && sqlx migrate revert

.PHONY: promote-admin
promote-admin: ## Make a user a platform superadmin, e.g. make promote-admin EMAIL=you@example.com
	@test -n "$(EMAIL)" || { echo "Usage: make promote-admin EMAIL=you@example.com"; exit 1; }
	@$(COMPOSE) exec -T db psql -U postgres -v ON_ERROR_STOP=1 -c \
	   "update users set is_superadmin = true where email = lower('$(EMAIL)');" | \
	   grep -q "UPDATE 1" && echo "Promoted $(EMAIL). Sign in again to pick up the flag." || \
	   { echo "No user with email $(EMAIL)."; exit 1; }

.PHONY: demote-admin
demote-admin: ## Take the platform superadmin flag away from a user
	@test -n "$(EMAIL)" || { echo "Usage: make demote-admin EMAIL=you@example.com"; exit 1; }
	@$(COMPOSE) exec -T db psql -U postgres -v ON_ERROR_STOP=1 -c \
	   "update users set is_superadmin = false where email = lower('$(EMAIL)');" | \
	   grep -q "UPDATE 1" && echo "Demoted $(EMAIL)." || \
	   { echo "No user with email $(EMAIL)."; exit 1; }

##@ Project

.PHONY: install-deps
install-deps: ## Install the development tooling
	@echo "Installing development dependencies..."
	@rustup component add rustfmt clippy
	@cargo install --locked sqlx-cli --no-default-features --features rustls,postgres
	@cargo install --locked cargo-audit
	@cd $(WEB_DIR) && npm install

.PHONY: shell
shell: ## Enter the Nix development shell (needs Nix with flakes)
	@if ! command -v nix &> /dev/null; then \
	   echo "nix not found. See https://nixos.org/download or use 'make install-deps' instead."; \
	   exit 1; \
	fi
	@echo "Entering the Nix development shell..."
	@env -u MAKEFLAGS -u MAKELEVEL -u MFLAGS nix develop

.PHONY: check-all
check-all: lint test lint-web check-web test-web ## Run every check that does not need containers

.PHONY: clean
clean: ## Remove the build artifacts
	@echo "Cleaning up..."
	@cargo clean
	@rm -rf $(WEB_DIR)/build $(WEB_DIR)/.svelte-kit

.PHONY: setup-hooks
setup-hooks: ## Install the Git hooks
	@echo "Setting up Git hooks..."
	@if ! command -v pre-commit &> /dev/null; then \
	   echo "pre-commit not found. Please install it using 'pip install pre-commit'"; \
	   exit 1; \
	fi
	@pre-commit install --hook-type pre-commit
	@pre-commit install --hook-type pre-push
	@pre-commit install-hooks

.PHONY: test-hooks
test-hooks: ## Run the Git hooks against every file
	@echo "Testing Git hooks..."
	@pre-commit run --all-files --show-diff-on-failure
