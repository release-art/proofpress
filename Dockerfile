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
RUN mkdir /tmp/itl && curl -fsSL ${TL_REPO}/install-tl-unx.tar.gz | tar -xz -C /tmp/itl --strip-components=1 \
 && printf 'selected_scheme scheme-infraonly\nTEXDIR /opt/texlive\nTEXMFLOCAL /opt/texlive/texmf-local\nTEXMFSYSVAR /opt/texlive/texmf-var\nTEXMFSYSCONFIG /opt/texlive/texmf-config\ninstopt_adjustpath 0\ntlpdbopt_install_docfiles 0\ntlpdbopt_install_srcfiles 0\n' > /tmp/p \
 && /tmp/itl/install-tl -profile /tmp/p -repository ${TL_REPO} \
 && rm -rf /tmp/itl /tmp/p
# aarch64-linux or x86_64-linux; one stable name for both.
RUN ln -s /opt/texlive/bin/$(uname -m)-linux /opt/texlive/bin/current
ENV PATH=/opt/texlive/bin/current:$PATH
COPY docker/texlive-packages.txt /tmp/texlive-packages.txt
RUN tlmgr option repository ${TL_REPO} && tlmgr install $(grep -v '^#' /tmp/texlive-packages.txt) \
 && fmtutil-sys --byfmt lualatex >/dev/null

# ---- veraPDF ----------------------------------------------------------------------
FROM ${DEBIAN_IMAGE} AS verapdf
ARG VERAPDF_VERSION=1.30.2
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates unzip openjdk-21-jre-headless \
 && rm -rf /var/lib/apt/lists/*
COPY docker/verapdf-auto-install.xml /tmp/auto-install.xml
RUN curl -fsSL https://software.verapdf.org/rel/1.30/verapdf-greenfield-${VERAPDF_VERSION}-installer.zip -o /tmp/v.zip \
 && unzip -q /tmp/v.zip -d /tmp/v && java -jar /tmp/v/*/verapdf-izpack-installer-*.jar /tmp/auto-install.xml \
 && rm -rf /tmp/v /tmp/v.zip

# ---- proof-pdf's venv, from the lockfile ------------------------------------------
FROM ${PYTHON_IMAGE} AS app
# The release workflow passes the tag; there is no .git in the build context to read it from.
ARG PKG_VERSION=0.0.0
ENV PDM_BUILD_SCM_VERSION=${PKG_VERSION}
RUN pip install --no-cache-dir pdm
WORKDIR /app
COPY pyproject.toml pdm.lock README.md LICENSE NOTICE /app/
COPY src /app/src
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
RUN apt-get update && apt-get install -y --no-install-recommends perl fontconfig ca-certificates \
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
    ghostscript openjdk-21-jre-headless perl fontconfig \
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
FROM ${PYTHON_IMAGE} AS test-venv
ARG PKG_VERSION=0.0.0
ENV PDM_BUILD_SCM_VERSION=${PKG_VERSION}
RUN pip install --no-cache-dir pdm
WORKDIR /app
COPY pyproject.toml pdm.lock README.md LICENSE NOTICE /app/
COPY src /app/src
RUN pdm install --frozen-lockfile --dev

FROM full AS test
COPY --from=test-venv /app /app
COPY tests /app/tests
WORKDIR /app
ENTRYPOINT ["pytest"]
CMD ["-q", "-p", "no:cacheprovider"]
