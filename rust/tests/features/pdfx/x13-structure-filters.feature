Feature: PDF/X-1a and PDF/X-3, data structure and compression
  ISO 15930-1 6.2 and 6.8, 15930-3 6.1 and 6.4, 15930-4 and 15930-6 6.1 and 6.5, with L-Q2 and L-Q4: features later
  than the part's PDF version are judged by the clause they would break.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | level | check |
      | x13-object-streams | 3:2002 | x13.structure.xref-table |
      | x1a-content-lzw | 1a:2001 | x1a.filter.listed-2001 |
      | x13-jpx-image | 1a:2003 | x13.filter.listed |
      | x13-jbig2-image | 3:2003 | x13.filter.no-jbig2 |
      | x13-image-16-bit | 3:2002 | x13.image.bit-depth |
      | x13-oc-off | 1a:2003 | x13.oc.hides-print |
      | x13-oc-on | 3:2003 | x13.oc.present |

  Scenario: ASCII85 is an encoding, not compression, and allowed at X-1a:2001
    Given the fixture "x1a-content-ascii85"
    When it is verified as PDF/X-1a:2001
    Then no check fails
