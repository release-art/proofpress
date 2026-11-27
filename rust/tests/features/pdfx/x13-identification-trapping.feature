Feature: PDF/X-1a and PDF/X-3, identification, dates and trapping in Info
  ISO 15930-1 6.9 and 6.13, 15930-3 6.5 and 6.6, 15930-4 and 15930-6 6.6 and 6.7: the version (and at X-1a:2001 the
  conformance), the dates, the title and the trapped state are Info entries at these levels, not XMP.

  Scenario Outline: x4-good identified as an older level raises nothing
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then no check fails

    Examples:
      | fixture | level |
      | x1a-2001-good | 1a:2001 |
      | x3-2002-good | 3:2002 |
      | x1a-2003-good | 1a:2003 |
      | x3-2003-good | 3:2003 |

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | level | check |
      | x13-trapped-missing | 3:2003 | x13.trapped.present |
      | x13-trapped-unknown | 1a:2001 | x13.trapped.true-or-false |
      | x13-trapnet-untrapped | 1a:2003 | x13.trapped.trapnet-implies-true |
      | x1a-trapnet-pcm-rgb | 1a:2001 | x1a.trapnet.pcm-cmyk |
      | x13-version-missing | 3:2002 | x13.identity.version-present |
      | x13-version-wrong | 1a:2003 | x13.identity.version-value |
      | x1a-conformance-misprint | 1a:2001 | x1a.identity.conformance |
      | x13-creation-date-missing | 3:2003 | x13.info.creation-date |
      | x13-mod-date-invalid | 1a:2001 | x13.info.mod-date |
      | x13-title-empty | 3:2002 | x13.info.title |
      | x13-producer-missing | 1a:2003 | x13.info.agent-names |
      | x13-xmp-mod-date-differs | 3:2003 | x13.xmp.mod-date-agrees |

  Scenario: A plain PDF/X-1 file is not evaluated as PDF/X-1a
    Given the fixture "x1a-plain-x1"
    When it is verified as PDF/X-1a:2001
    Then check "x1a.identity.conformance" is not evaluated
