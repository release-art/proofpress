# Two images from one file:
#   lualatex  TeX Live 2025 (lualatex + a general package set) on Debian. No Java, no Python.
#   full      lualatex's TeX tree + Ghostscript + veraPDF + the proof-pdf CLI.
#
#   docker build --target lualatex -t releaseart/lualatex .
#   docker build --target full     -t releaseart/full .
#
# Bases are pinned by digest and bumped deliberately.
ARG DEBIAN_IMAGE=debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
ARG PYTHON_IMAGE=python:3.14-slim@sha256:51dafde81dbdb6ebde285137a295cf18a47ca95234fe388a343719cb97305b3d
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
# fontconfig for luaotfload; perl because several TeX Live scripts (tlmgr, latexmk) need it.
RUN apt-get update && apt-get install -y --no-install-recommends perl fontconfig ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=tex /opt/texlive /opt/texlive
# Read-only-root friendly: HOME=/tmp takes the luaotfload cache to the scratch volume.
ENV PATH=/opt/texlive/bin/current:$PATH HOME=/tmp
WORKDIR /work
CMD ["lualatex", "--version"]

# ---- full -------------------------------------------------------------------------
FROM ${PYTHON_IMAGE} AS full
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="releaseart/full" \
      org.opencontainers.image.source="https://github.com/release-art/proofpress" \
      org.opencontainers.image.licenses="Apache-2.0 AND AGPL-3.0-or-later AND LPPL-1.3c AND GPL-2.0-or-later" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
# Ghostscript (AGPL; source via `apt-get source ghostscript`) and Java for veraPDF.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ghostscript openjdk-21-jre-headless perl fontconfig \
 && rm -rf /var/lib/apt/lists/*
COPY --from=tex /opt/texlive /opt/texlive
COPY --from=verapdf /opt/verapdf /opt/verapdf
RUN ln -s /opt/verapdf/verapdf /usr/local/bin/verapdf
COPY --from=app /app/.venv /app/.venv
COPY LICENSE NOTICE /usr/share/doc/proof-pdf/
ENV PATH=/app/.venv/bin:/opt/texlive/bin/current:$PATH HOME=/tmp PYTHONUNBUFFERED=1
WORKDIR /work
ENTRYPOINT ["proof-pdf"]
CMD ["--help"]
