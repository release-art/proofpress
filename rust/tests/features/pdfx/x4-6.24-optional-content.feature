Feature: PDF/X-4 6.24, optional content
  Every configuration is named, uniquely; an /Order lists every group or none (X4-Q13); no configuration has /AS.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-oc-unnamed | x4.oc.config-name-present |
      | x4-oc-same-name | x4.oc.config-name-unique |
      | x4-oc-order-incomplete | x4.oc.order-complete |
      | x4-oc-as | x4.oc.no-as |
