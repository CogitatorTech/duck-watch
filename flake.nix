{
  description = "DuckWatch: An observability tool for MotherDuck";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            };
          in
          f pkgs
        );
    in
    {
      devShells = forAllSystems (pkgs:
        let
          # Use the exact toolchain from rust-toolchain.toml so the shell, the
          # Makefile, and CI all agree on the compiler version.
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            name = "duck-watch-dev";

            packages = [
              rustToolchain

              # Backend tooling the Makefile calls
              pkgs.sqlx-cli
              pkgs.cargo-audit

              # Frontend tooling; the major version matches the one used in CI
              pkgs.nodejs_24

              # Provides psql for inspecting the database container
              pkgs.postgresql_18

              # Drives docker-compose.yaml; the Makefile falls back to this
              pkgs.docker-compose

              # Shared tooling
              pkgs.gnumake
              pkgs.pre-commit

              # For the optional Python tooling environment in pyproject.toml
              pkgs.python3
              pkgs.uv

              # Native build inputs that crates commonly need
              pkgs.pkg-config
              pkgs.openssl
            ];

            # Matches the `db` service in docker-compose.yaml, so sqlx-cli and the
            # integration tests work in this shell without further setup.
            DATABASE_URL = "postgres://postgres:postgres@localhost:5432/postgres?sslmode=disable";

            shellHook = ''
              echo "=========================================================="
              echo "  DuckWatch development environment"
              echo "  rust: $(rustc --version)"
              echo "  node: $(node --version)"
              echo "  psql: $(psql --version | cut -d' ' -f3)"
              echo ""
              echo "  make help       show the available targets"
              echo "  make docker-up  start PostgreSQL"
              echo "=========================================================="
            '';
          };
        });

      # `nixfmt` is the formatter that nixpkgs itself uses.
      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
