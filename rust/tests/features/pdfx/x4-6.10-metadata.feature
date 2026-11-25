Feature: PDF/X-4 6.10, metadata and file identifiers
  The catalog has XMP that names its namespaces with Table 2's prefixes; no metadata stream is filtered, has a bytes
  or encoding header, or is malformed; Info entries are mirrored in XMP; the file identifiers and dates are there.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-xmp-filtered | x4.xmp.unfiltered |
      | x4-page-xmp-filtered | x4.xmp.unfiltered |
      | x4-page-xmp-malformed | x4.xmp.well-formed |
      | x4-page-xmp-header-bytes | x4.xmp.no-bytes-encoding-attrs |
      | x4-xmp-prefix | x4.xmp.namespace-prefixes |
      | x4-xmp-no-document-id | x4.xmp.document-id |
      | x4-xmp-document-id-not-uuid | x4.xmp.document-id-uuid |
      | x4-xmp-no-version-id | x4.xmp.version-id |
      | x4-xmp-no-rendition-class | x4.xmp.rendition-class |
      | x4-xmp-no-metadata-date | x4.xmp.metadata-date |
      | x4-xmp-no-title | x4.xmp.title |
      | x4-xmp-no-creator-tool | x4.xmp.agent-names |
      | x4-info-version-utf16 | x4.info.pdfx-version-pdfdoc |
      | x4-info-title-differs | x4.info.xmp-analogue-equal |
      | x4-info-no-analogue | x4.info.xmp-analogue-present |
      | x4-author-bag | x4.info.author-single-creator |
      | x4-no-trailer-id | x4.trailer.id |

  Scenario: Without document XMP, every property it must hold is missing
    Given the fixture "x4-no-xmp"
    When it is verified as PDF/X-4
    Then check "x4.xmp.present" fails
    And check "x4.xmp.document-id" fails
    And check "x4.info.xmp-analogue-present" fails

  Scenario: An invalid date fails, and cannot equal Info's
    Given the fixture "x4-xmp-bad-create-date"
    When it is verified as PDF/X-4
    Then check "x4.xmp.create-date" fails
    And check "x4.info.xmp-analogue-equal" fails
