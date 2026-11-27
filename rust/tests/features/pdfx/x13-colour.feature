Feature: PDF/X-1a and PDF/X-3, the output intent and print-element colour
  ISO 15930-1 6.3, 15930-3 6.2, 15930-4 6.2 and 15930-6 6.2: when the output profile must be embedded, what it must
  be, and which colour spaces print elements may use at X-1a and X-3.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | level | check |
      | x13-icc-name-no-registry | 1a:2001 | x13.output-intent.icc-name-needs-registry |
      | x13-no-profile-unregistered-2003 | 3:2003 | x13.output-intent.dest-profile-by-registry |
      | x13-no-profile-unregistered-2002 | 3:2002 | x13.output-intent.dest-profile-by-icc-name |
      | x3-no-profile-icc-colour | 3:2002 | x3.output-intent.dest-profile-for-colour |
      | x1a-profile-no-atob1 | 1a:2001 | x1a.output-intent.profile-atob1 |
      | x1a-intent-rgb | 1a:2003 | x1a.output-intent.cmyk |
      | x1a-no-output-condition | 1a:2001 | x1a.output-intent.output-condition-unregistered |
      | x3-intent-no-info | 3:2002 | x3.output-intent.info |
      | x13-profile-v4 | 3:2003 | x13.output-intent.profile-version |
      | x3-icc-v4 | 3:2003 | x3.icc.profile-version |
      | x3-2003-registry-https | 3:2003 | x3.output-intent.registry-name-color-org-2003 |
      | x1a-2003-registry-slash | 1a:2003 | x4.output-intent.registry-name-color-org |
      | x1a-device-rgb | 1a:2001 | x1a.colour.print-spaces |
      | x1a-default-cmyk | 1a:2003 | x1a.colour.print-spaces |
      | x1a-spot-rgb-alternate | 1a:2003 | x1a.spot.alternate-cmyk-or-gray |
      | x1a-unused-default | 1a:2001 | x1a.colour.no-default-space |

  Scenario Outline: What the older parts allow passes
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then no check fails

    Examples:
      | fixture | level |
      | x3-2002-registered-no-profile | 3:2002 |
      | x3-2003-registry-slash | 3:2003 |
