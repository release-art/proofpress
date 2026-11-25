Feature: PDF/X-4 6.11, identification
  XMP declares exactly PDF/X-4, and nothing else in the pdfxid namespace; Info has no GTS_PDFXConformance.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-xmp-no-version | x4.identity.version-present |
      | x4-xmp-wrong-version | x4.identity.version-value |
      | x4-xmp-pdfxid-extra | x4.identity.pdfxid-only-version |
      | x4-info-conformance | x4.identity.no-info-conformance |
