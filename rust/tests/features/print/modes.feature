Feature: The job's colour mode
  Transparency declares its blend space, in the output intent's family; spot inks are on the allowed list; a CMYK job
  has no RGB images (RGB vector artwork is advisory), an RGB job no CMYK artwork; embedded ICC profiles are sound and
  agree with the job.

  Scenario: Transparency with a matching blend space
    Given the fixture "transparent-matching"
    When it is verified for print
    Then it passes

  Scenario: Transparency blending in the wrong family
    Given the fixture "transparent-mismatch"
    When it is verified for print
    Then check "blend-space" fails on page 1

  Scenario: A spot ink not on the list
    Given the fixture "spot-ink"
    When it is verified for print with '{"allow_spots": []}'
    Then check "spot-ink" fails on page 1

  Scenario: An RGB image in a CMYK job
    Given the fixture "rgb-image"
    When it is verified for print
    Then check "colour-space" fails on page 1

  Scenario: CMYK artwork in an RGB job
    Given the fixture "cmyk-vector"
    When it is verified for print with '{"colour_mode": "rgb"}'
    Then check "rgb-only" fails on page 1

  Scenario: Two RGB profiles in an RGB job
    Given the fixture "mixed-rgb-profiles"
    When it is verified for print with '{"colour_mode": "rgb"}'
    Then check "icc-profile" fails
