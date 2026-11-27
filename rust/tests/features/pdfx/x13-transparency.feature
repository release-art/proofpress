Feature: PDF/X-1a and PDF/X-3, transparency
  ISO 15930-4 and 15930-6 6.16 ban soft masks, opacity, blend modes other than Normal and Compatible, and transparency
  groups on forms; L-Q3 bans the first three at X-1a:2001 and X-3:2002 (15930-3 Annex D).

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | level | check |
      | x13-smask-extgstate | 1a:2001 | x13.transparency.smask |
      | x13-image-smask | 3:2003 | x13.transparency.smask |
      | x13-opacity | 3:2002 | x13.transparency.extgstate-values |
      | x13-blend-multiply | 1a:2003 | x13.transparency.extgstate-values |
      | x13-form-group | 3:2003 | x13.transparency.no-group |

  Scenario Outline: Opaque values and a page group pass
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then no check fails

    Examples:
      | fixture | level |
      | x13-opaque-transparency-keys | 1a:2001 |
      | x13-page-group | 1a:2003 |
