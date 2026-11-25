Feature: PDF/X-4 6.4.3.4 and 6.20, spot colours and transparency
  Spots have /Colorants entries, one definition per name, a /Process space of the intent's family, no /DotGain, and
  consistent printing orders and solidities; group spaces obey 6.4.3 and blend modes are the PDF Reference's.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-devicen-no-colorants | x4.spot.colorants-entry |
      | x4-devicen-process-rgb | x4.spot.process-colorspace |
      | x4-nchannel-dotgain | x4.spot.no-dotgain |
      | x4-spot-confusing-name | x4.spot.confusing-names |
      | x4-spot-two-definitions | x4.spot.consistent-definition |
      | x4-printing-order-clash | x4.spot.printing-order-consistent |
      | x4-solidities-clash | x4.spot.solidities-consistent |
      | x4-group-rgb | x4.transparency.group-cs-restricted |
      | x4-blend-mode-unknown | x4.transparency.blend-modes |
