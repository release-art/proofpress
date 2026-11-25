# Two images from one file:
#   lualatex  TeX Live 2025 (lualatex + a general package set) on Debian. No Java, no Python.
#   lualatex-python  Python + lualatex's TeX tree + Ghostscript + veraPDF: everything proof-pdf needs, a base for pip-installing it.
#   full      lualatex-python + the proof-pdf CLI.
#
#   docker build --target lualatex -t releaseart/lualatex .
#   docker build --target full     -t releaseart/full .
#
# Bases float on their tags so rebuilds pick up security fixes; bin/docker/base-digests.sh logs
# what they resolved to (CI runs it before every build).
ARG DEBIAN_IMAGE=debian:trixie-slim
ARG PYTHON_IMAGE=python:3.15-slim
ARG TL_REPO=https://ftp.math.utah.edu/pub/tex/historic/systems/texlive/2025/tlnet-final

# ---- TeX Live from the frozen historic mirror (reproducible) --------------------
FROM ${DEBIAN_IMAGE} AS tex
ARG TL_REPO
RUN apt-get update && apt-get install -y --no-install-recommends perl curl xz-utils ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN mkdir /tmp/itl && curl -fsSL --retry 5 --retry-delay 10 ${TL_REPO}/install-tl-unx.tar.gz | tar -xz -C /tmp/itl --strip-components=1 \
 && printf 'selected_scheme scheme-infraonly\nTEXDIR /opt/texlive\nTEXMFLOCAL /opt/texlive/texmf-local\nTEXMFSYSVAR /opt/texlive/texmf-var\nTEXMFSYSCONFIG /opt/texlive/texmf-config\ninstopt_adjustpath 0\ntlpdbopt_install_docfiles 0\ntlpdbopt_install_srcfiles 0\n' > /tmp/p \
 && /tmp/itl/install-tl -profile /tmp/p -repository ${TL_REPO} \
 && rm -rf /tmp/itl /tmp/p
# aarch64-linux or x86_64-linux; one stable name for both.
RUN ln -s /opt/texlive/bin/$(uname -m)-linux /opt/texlive/bin/current
ENV PATH=/opt/texlive/bin/current:$PATH
COPY docker/texlive-packages.txt /tmp/texlive-packages.txt
# The mirror drops downloads from CI runners now and then; tlmgr then carries on without the package and fails later in
# fmtutil. Re-running the install fetches only what is missing, so retry before giving up.
RUN tlmgr option repository ${TL_REPO} \
 && for i in 1 2 3 4; do tlmgr install $(grep -v '^#' /tmp/texlive-packages.txt) && break || { [ "$i" = 4 ] && exit 1; sleep 30; }; done \
 && fmtutil-sys --byfmt lualatex >/dev/null

# ---- veraPDF ----------------------------------------------------------------------
FROM ${DEBIAN_IMAGE} AS verapdf
ARG VERAPDF_VERSION=1.30.2
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates unzip openjdk-21-jre-headless \
 && rm -rf /var/lib/apt/lists/*
COPY docker/verapdf-auto-install.xml /tmp/auto-install.xml
# --retry covers the 502s these hosts return now and then.
RUN curl -fsSL --retry 5 --retry-delay 10 https://software.verapdf.org/rel/1.30/verapdf-greenfield-${VERAPDF_VERSION}-installer.zip -o /tmp/v.zip \
 && unzip -q /tmp/v.zip -d /tmp/v && java -jar /tmp/v/*/verapdf-izpack-installer-*.jar /tmp/auto-install.xml \
 && rm -rf /tmp/v /tmp/v.zip

# ---- Rust and pdm, to build proof-pdf's extension -------------------------------------
# maturin builds proof_pdf._internal._core from rust/ with the toolchain pinned in rust/rust-toolchain.toml.
FROM ${PYTHON_IMAGE} AS builder
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates build-essential \
 && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL --retry 5 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
COPY rust/rust-toolchain.toml /tmp/rust-toolchain.toml
# maturin runs cargo from /app, where rustup cannot see rust/rust-toolchain.toml, so the pinned toolchain is the default.
RUN cd /tmp && rustup toolchain install && rustup default "$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)" \
 && rm rust-toolchain.toml
RUN pip install --no-cache-dir pdm
WORKDIR /app
# The release workflow passes the tag; there is no .git in the build context to read it from.
ARG PKG_VERSION=0.0.0
COPY pyproject.toml pdm.lock README.md LICENSE NOTICE /app/
COPY rust /app/rust
COPY bin/rust/set-version.sh /app/bin/rust/set-version.sh
COPY src /app/src
RUN bin/rust/set-version.sh "${PKG_VERSION}"

# ---- proof-pdf's venv, from the lockfile ------------------------------------------
FROM builder AS app
RUN pdm install --frozen-lockfile --prod --no-editable

# ---- lualatex ---------------------------------------------------------------------
FROM ${DEBIAN_IMAGE} AS lualatex
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="releaseart/lualatex" \
      org.opencontainers.image.source="https://github.com/release-art/proofpress" \
      org.opencontainers.image.licenses="LPPL-1.3c AND GPL-2.0-or-later" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
ENV PROOFPRESS_VERSION=${VERSION} PROOFPRESS_REVISION=${REVISION}
# fontconfig for luaotfload; perl because several TeX Live scripts (tlmgr, latexmk) need it.
# curl and xz-utils: tlmgr downloads .tar.xz archives with them, and without them `tlmgr install` fails every download
# yet still exits 0 -- so a derived image that adds packages would build fine and miss them at typeset time.
RUN apt-get update && apt-get install -y --no-install-recommends perl fontconfig ca-certificates curl xz-utils \
 && rm -rf /var/lib/apt/lists/*
COPY --from=tex /opt/texlive /opt/texlive
# Read-only-root friendly: HOME=/tmp takes the luaotfload cache to the scratch volume.
ENV PATH=/opt/texlive/bin/current:$PATH HOME=/tmp
WORKDIR /work
CMD ["lualatex", "--version"]

# ---- lualatex-python ----------------------------------------------------------------
# Everything proof-pdf needs except proof-pdf: Python, LuaLaTeX, Ghostscript (AGPL; source via `apt-get source ghostscript`)
# and veraPDF with its JRE. A base for images that pip-install proof-pdf (or their own packages); `full` is this plus the CLI.
FROM ${PYTHON_IMAGE} AS lualatex-python
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="releaseart/lualatex-python" \
      org.opencontainers.image.source="https://github.com/release-art/proofpress" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later AND LPPL-1.3c AND GPL-2.0-or-later" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
ENV PROOFPRESS_VERSION=${VERSION} PROOFPRESS_REVISION=${REVISION}
RUN apt-get update && apt-get install -y --no-install-recommends \
    ghostscript openjdk-21-jre-headless perl fontconfig curl xz-utils \
 && rm -rf /var/lib/apt/lists/*
COPY --from=tex /opt/texlive /opt/texlive
COPY --from=verapdf /opt/verapdf /opt/verapdf
RUN ln -s /opt/verapdf/verapdf /usr/local/bin/verapdf
ENV PATH=/opt/texlive/bin/current:$PATH HOME=/tmp
WORKDIR /work
CMD ["lualatex", "--version"]

# ---- full -------------------------------------------------------------------------
FROM lualatex-python AS full
LABEL org.opencontainers.image.title="releaseart/full" \
      org.opencontainers.image.licenses="Apache-2.0 AND AGPL-3.0-or-later AND LPPL-1.3c AND GPL-2.0-or-later"
COPY --from=app /app/.venv /app/.venv
COPY LICENSE NOTICE /usr/share/doc/proof-pdf/
ENV PATH=/app/.venv/bin:$PATH PYTHONUNBUFFERED=1
ENTRYPOINT ["proof-pdf"]
CMD ["--help"]

# ---- test (never published) ---------------------------------------------------------
# The suite inside the real image: the dev dependencies on top of `full`, with real veraPDF and Ghostscript, so the
# integration tests run instead of skipping. bin/docker/test.sh builds and runs it.
FROM builder AS test-venv
RUN pdm install --frozen-lockfile --dev

FROM full AS test
COPY --from=test-venv /app /app
COPY tests /app/tests
WORKDIR /app
ENTRYPOINT ["pytest"]
CMD ["-q", "-p", "no:cacheprovider"]
