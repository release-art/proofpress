Feature: PDF/X-4 6.4.3, element colour spaces
  Device spaces match the output intent (DeviceGray also under CMYK), spot alternates obey the same rule, ICC profiles
  are version 2 or 4, an overprinting four-colour ICCBased paint has OPM 0, and none uses the intent's own profile.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails on page 1
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-device-rgb | x4.colour.device-space-matches-intent |
      | x4-spot-rgb-alternate | x4.spot.alternate-space-restricted |
      | x4-icc-v5 | x4.icc.profile-version |
      | x4-icc-opm | x4.icc.overprint-mode-zero |
      | x4-icc-intent-profile | x4.icc.cmyk-not-intent-profile |
      | x4-icc-intent-copy | x4.icc.cmyk-not-intent-profile |
